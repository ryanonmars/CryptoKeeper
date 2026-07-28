use std::collections::HashMap;
use std::io::{self, ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use chrono::Utc;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use termkey::native::site::{
    entry_authorizes_origin, entry_fingerprint, find_site_matches as authorized_site_matches,
    HttpsOrigin, SiteMatch, NATIVE_CAPABILITIES, NATIVE_PROTOCOL_VERSION,
};
use termkey::vault::model::{Entry, SecretType};
use termkey::{apply_configured_vault_dir_override, config, crypto, vault};
use zeroize::{Zeroize, Zeroizing};

const MAX_NATIVE_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_NATIVE_RESPONSE_BYTES: usize = 900 * 1024;
const ISSUED_MATCH_TTL: Duration = Duration::from_secs(30);
const MAX_ISSUED_MATCHES: usize = 100;

struct SensitiveString(Zeroizing<String>);

impl<'de> Deserialize<'de> for SensitiveString {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).map(|value| Self(Zeroizing::new(value)))
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum NativeRequest {
    Ping {
        #[serde(default)]
        #[serde(alias = "protocolVersion")]
        protocol_version: Option<u32>,
    },
    Status {
        #[serde(default)]
        #[serde(alias = "protocolVersion")]
        protocol_version: Option<u32>,
    },
    GeneratePassword,
    GetAutofillEntry {
        id: String,
        origin: String,
        #[serde(default)]
        #[serde(alias = "secondaryPassword")]
        secondary_password: Option<SensitiveString>,
    },
    FindSiteMatches {
        url: String,
    },
    SavePasswordEntry {
        name: String,
        #[serde(default)]
        username: Option<String>,
        password: SensitiveString,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        #[serde(alias = "secondaryPassword")]
        secondary_password: Option<SensitiveString>,
    },
    ListEntries,
    Unlock {
        password: SensitiveString,
    },
}

#[derive(Clone)]
struct IssuedMatch {
    fingerprint: String,
    origin: String,
    expires_at: Instant,
}

#[derive(Default)]
struct HostState {
    session: Option<vault::session::VaultSession>,
    issued_matches: HashMap<String, IssuedMatch>,
    protocol_negotiated: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    app: &'static str,
    version: &'static str,
    vault_path: String,
    vault_exists: bool,
    first_run_complete: bool,
    recovery_configured: bool,
    locked: bool,
    protocol_version: u32,
    capabilities: &'static [&'static str],
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct EntrySummary {
    id: String,
    name: String,
    secret_type: String,
    network: String,
    has_secondary_password: bool,
    public_address: Option<String>,
    username: Option<String>,
    url: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SiteMatchesResponse {
    site_url: String,
    site_origin: String,
    site_hostname: String,
    matches: Vec<SiteMatch>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AutofillEntryResponse {
    id: String,
    name: String,
    username: Option<String>,
    password: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum NativeResponse {
    Pong {
        app: &'static str,
        version: &'static str,
        protocol_version: u32,
        capabilities: &'static [&'static str],
    },
    Status(StatusResponse),
    GeneratedPassword {
        password: String,
    },
    AutofillEntry {
        entry: AutofillEntryResponse,
    },
    SaveEntry {
        entry_name: String,
    },
    SiteMatches(SiteMatchesResponse),
    ListEntries {
        entries: Vec<EntrySummary>,
    },
    Unlock {
        unlocked: bool,
        #[serde(rename = "recoveryNotice", skip_serializing_if = "Option::is_none")]
        recovery_notice: Option<String>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeWireResponse {
    request_id: String,
    #[serde(flatten)]
    response: NativeResponse,
}

impl NativeWireResponse {
    fn zeroize_secrets(&mut self) {
        match &mut self.response {
            NativeResponse::GeneratedPassword { password } => password.zeroize(),
            NativeResponse::AutofillEntry { entry } => entry.password.zeroize(),
            _ => {}
        }
    }
}

fn load_status_for_state(state: &HostState) -> StatusResponse {
    let config = config::load_config().unwrap_or_default();
    let vault_exists = vault::storage::vault_exists();

    StatusResponse {
        app: "termkey",
        version: env!("CARGO_PKG_VERSION"),
        vault_path: vault::storage::vault_path().display().to_string(),
        vault_exists,
        first_run_complete: config.first_run_complete,
        recovery_configured: config.recovery.is_some(),
        locked: vault_exists && state.session.is_none(),
        protocol_version: NATIVE_PROTOCOL_VERSION,
        capabilities: NATIVE_CAPABILITIES,
    }
}

fn read_message(reader: &mut impl Read) -> io::Result<Option<Zeroizing<Vec<u8>>>> {
    let mut len_buf = [0_u8; 4];
    let mut read_len = 0;

    while read_len < len_buf.len() {
        let bytes_read = reader.read(&mut len_buf[read_len..])?;
        if bytes_read == 0 {
            if read_len == 0 {
                return Ok(None);
            }

            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "native host payload length prefix was truncated",
            ));
        }

        read_len += bytes_read;
    }

    let payload_len = u32::from_le_bytes(len_buf) as usize;
    if payload_len > MAX_NATIVE_MESSAGE_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "native host payload exceeds 64 MiB",
        ));
    }
    let mut payload = Zeroizing::new(vec![0_u8; payload_len]);
    reader.read_exact(&mut payload)?;
    Ok(Some(payload))
}

#[cfg(test)]
fn write_message(writer: &mut impl Write, response: &impl Serialize) -> io::Result<()> {
    let payload = serde_json::to_vec(response).map_err(io::Error::other)?;
    write_payload(writer, &payload)
}

fn encode_wire_response(response: &NativeWireResponse) -> io::Result<Zeroizing<Vec<u8>>> {
    if let Some(payload) = serialize_bounded_zeroizing(response)? {
        return Ok(payload);
    }

    serialize_bounded_zeroizing(&NativeWireResponse {
        request_id: response.request_id.clone(),
        response: NativeResponse::Error {
            message: "Native response exceeded the safe browser message limit.".to_string(),
        },
    })
    .and_then(|payload| {
        payload.ok_or_else(|| {
            io::Error::new(
                ErrorKind::InvalidData,
                "compact native response exceeded the safe browser message limit",
            )
        })
    })
}

struct BoundedZeroizingWriter {
    payload: Zeroizing<Vec<u8>>,
    exceeded: bool,
}

impl BoundedZeroizingWriter {
    fn new() -> Self {
        Self {
            payload: Zeroizing::new(Vec::with_capacity(MAX_NATIVE_RESPONSE_BYTES)),
            exceeded: false,
        }
    }
}

impl Write for BoundedZeroizingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let fits = self
            .payload
            .len()
            .checked_add(bytes.len())
            .is_some_and(|length| length <= MAX_NATIVE_RESPONSE_BYTES);
        if !fits {
            self.exceeded = true;
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "native response exceeded the safe browser message limit",
            ));
        }
        self.payload.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialize_bounded_zeroizing(value: &impl Serialize) -> io::Result<Option<Zeroizing<Vec<u8>>>> {
    let mut writer = BoundedZeroizingWriter::new();
    match serde_json::to_writer(&mut writer, value) {
        Ok(()) => Ok(Some(writer.payload)),
        Err(_) if writer.exceeded => Ok(None),
        Err(err) => Err(io::Error::other(err)),
    }
}

fn write_wire_message(
    writer: &mut impl Write,
    response: &mut NativeWireResponse,
) -> io::Result<()> {
    let payload = encode_wire_response(response);
    response.zeroize_secrets();
    let payload = payload?;
    write_payload(writer, &payload)
}

fn write_payload(writer: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

fn unlock_vault(state: &mut HostState, password: SensitiveString) -> NativeResponse {
    if !vault::storage::vault_exists() {
        return NativeResponse::Error {
            message: "Vault not found. Run `termkey init` first.".to_string(),
        };
    }

    match vault::session::VaultSession::open(password.0, vault::storage::vault_path()) {
        Ok(outcome) => {
            let recovery_notice = outcome.recovery_notice;
            state.issued_matches.clear();
            state.session = Some(outcome.session);
            NativeResponse::Unlock {
                unlocked: true,
                recovery_notice,
            }
        }
        Err(err) => NativeResponse::Error {
            message: err.to_string(),
        },
    }
}

fn require_unlocked_session(
    state: &HostState,
) -> Result<&vault::session::VaultSession, NativeResponse> {
    state.session.as_ref().ok_or_else(|| NativeResponse::Error {
        message: "Vault is locked. Unlock it first.".to_string(),
    })
}

fn summarize_entry(index: usize, entry: &Entry) -> EntrySummary {
    EntrySummary {
        id: (index + 1).to_string(),
        name: entry.name.clone(),
        secret_type: match &entry.secret_type {
            SecretType::PrivateKey => "Private Key".to_string(),
            SecretType::SeedPhrase => "Seed Phrase".to_string(),
            SecretType::Password => "Password".to_string(),
            SecretType::Other(label) => {
                if label.trim().is_empty() {
                    "Other".to_string()
                } else {
                    label.trim().to_string()
                }
            }
        },
        network: entry.network.clone(),
        has_secondary_password: entry.has_secondary_password,
        public_address: entry.public_address.clone(),
        username: entry.username.clone(),
        url: entry.url.clone(),
    }
}

fn find_site_matches(state: &mut HostState, site_url: String) -> NativeResponse {
    let origin = match HttpsOrigin::parse(&site_url) {
        Ok(origin) => origin,
        Err(_) => {
            return NativeResponse::Error {
                message: "Current tab origin is not a supported HTTP or HTTPS origin.".to_string(),
            }
        }
    };

    let matches = {
        let session = match require_unlocked_session(state) {
            Ok(session) => session,
            Err(response) => return response,
        };
        authorized_site_matches(&session.vault, &origin)
    };
    let site_hostname = url::Url::parse(origin.as_str())
        .expect("site origin always contains a parsed URL")
        .host_str()
        .expect("site origin always contains a host")
        .to_string();
    let now = Instant::now();
    state.issued_matches.clear();
    let matches = matches
        .into_iter()
        .take(MAX_ISSUED_MATCHES)
        .map(|mut site_match| {
            let fingerprint = std::mem::take(&mut site_match.id);
            let id = issue_match_id(&state.issued_matches);
            state.issued_matches.insert(
                id.clone(),
                IssuedMatch {
                    fingerprint,
                    origin: origin.as_str().to_string(),
                    expires_at: now + ISSUED_MATCH_TTL,
                },
            );
            site_match.id = id;
            site_match
        })
        .collect();

    NativeResponse::SiteMatches(SiteMatchesResponse {
        site_url: origin.as_str().to_string(),
        site_origin: origin.as_str().to_string(),
        site_hostname,
        matches,
    })
}

fn issue_match_id(existing: &HashMap<String, IssuedMatch>) -> String {
    loop {
        let mut bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let id = hex::encode(bytes);
        if !existing.contains_key(&id) {
            return id;
        }
    }
}

fn list_entries(state: &HostState) -> NativeResponse {
    let session = match require_unlocked_session(state) {
        Ok(session) => session,
        Err(response) => return response,
    };

    let entries = session
        .vault
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| summarize_entry(index, entry))
        .collect();

    NativeResponse::ListEntries { entries }
}

fn get_autofill_entry(
    state: &mut HostState,
    id: String,
    origin: String,
    secondary_password: Option<SensitiveString>,
) -> NativeResponse {
    let origin = match HttpsOrigin::parse(&origin) {
        Ok(origin) => origin,
        Err(_) => {
            return NativeResponse::Error {
                message: "Current tab origin is not a supported HTTP or HTTPS origin.".to_string(),
            }
        }
    };
    if state.session.is_none() {
        return NativeResponse::Error {
            message: "Vault is locked. Unlock it first.".to_string(),
        };
    }
    if !is_valid_request_id(&id) {
        return NativeResponse::Error {
            message: "Selected login match ID is invalid.".to_string(),
        };
    }
    let issued = match state.issued_matches.get(&id).cloned() {
        Some(issued) if issued.origin == origin.as_str() && issued.expires_at > Instant::now() => {
            issued
        }
        _ => {
            state.issued_matches.remove(&id);
            return NativeResponse::Error {
                message: "Selected login match expired or is no longer valid.".to_string(),
            };
        }
    };

    let reload_result = match state.session.as_mut() {
        Some(session) => session.reload(),
        None => {
            return NativeResponse::Error {
                message: "Vault is locked. Unlock it first.".to_string(),
            }
        }
    };
    if let Err(err) = reload_result {
        state.session = None;
        state.issued_matches.clear();
        return NativeResponse::Error {
            message: err.to_string(),
        };
    }

    let session = match require_unlocked_session(state) {
        Ok(session) => session,
        Err(response) => return response,
    };

    let entry = match find_unique_entry_by_fingerprint(&session.vault, &issued.fingerprint) {
        Ok(entry) => entry,
        Err(()) => {
            return NativeResponse::Error {
                message: "Selected login match changed, is ambiguous, or no longer exists."
                    .to_string(),
            }
        }
    };

    if entry.secret_type != SecretType::Password {
        return NativeResponse::Error {
            message: "Selected entry is not a password entry.".to_string(),
        };
    }

    if !entry_authorizes_origin(entry, &origin) {
        return NativeResponse::Error {
            message: "Selected entry is not authorized for the current site origin.".to_string(),
        };
    }

    let secondary_password = secondary_password.map(|password| password.0);
    let secret = match entry.reveal_secret(
        secondary_password
            .as_ref()
            .map(|password| password.as_str()),
    ) {
        Ok(secret) => secret,
        Err(err) => {
            let message = if entry.has_secondary_password && secondary_password.is_none() {
                "This entry requires a secondary password to view.".to_string()
            } else {
                err.to_string()
            };
            return NativeResponse::Error { message };
        }
    };

    let response = NativeResponse::AutofillEntry {
        entry: AutofillEntryResponse {
            id: id.clone(),
            name: entry.name.clone(),
            username: entry.username.clone(),
            password: secret.to_string(),
        },
    };
    state.issued_matches.remove(&id);
    response
}

fn find_unique_entry_by_fingerprint<'a>(
    vault: &'a vault::model::VaultData,
    fingerprint: &str,
) -> std::result::Result<&'a Entry, ()> {
    let mut matches = vault
        .entries
        .iter()
        .filter(|entry| entry_fingerprint(entry) == fingerprint);
    let entry = matches.next().ok_or(())?;
    if matches.next().is_some() {
        return Err(());
    }
    Ok(entry)
}

fn normalize_optional_field(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_optional_secret(value: Option<SensitiveString>) -> Option<Zeroizing<String>> {
    let value = value?;
    let trimmed = value.0.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() == value.0.len() {
        return Some(value.0);
    }
    Some(Zeroizing::new(trimmed.to_string()))
}

fn build_password_entry(
    name: String,
    username: Option<String>,
    password: SensitiveString,
    url: Option<String>,
    secondary_password: Option<SensitiveString>,
) -> Result<Entry, NativeResponse> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(NativeResponse::Error {
            message: "Enter a name for this login.".to_string(),
        });
    }

    if password.0.is_empty() {
        return Err(NativeResponse::Error {
            message: "Password cannot be empty.".to_string(),
        });
    }

    let now = Utc::now();
    let username = normalize_optional_field(username);
    let url = url
        .map(|url| {
            if url.is_empty() {
                return Ok(None);
            }
            HttpsOrigin::parse(&url)
                .map(|origin| origin.as_str().to_string())
                .map_err(|_| NativeResponse::Error {
                    message: "Saved login URL must be a canonical HTTP or HTTPS origin."
                        .to_string(),
                })
                .map(Some)
        })
        .transpose()?
        .flatten();
    let secondary_password = normalize_optional_secret(secondary_password);

    let (
        has_secondary_password,
        secret,
        entry_key_wrapped,
        entry_key_nonce,
        entry_key_salt,
        encrypted_secret,
        encrypted_secret_nonce,
    ) = if let Some(secondary_password) = secondary_password {
        let entry_key = crypto::entry_key::generate_entry_key();
        let (encrypted_secret, encrypted_secret_nonce) =
            crypto::entry_key::encrypt_secret(&entry_key, &password.0).map_err(|err| {
                NativeResponse::Error {
                    message: err.to_string(),
                }
            })?;
        let (entry_key_wrapped, entry_key_nonce, entry_key_salt) =
            crypto::entry_key::wrap_entry_key(&entry_key, &secondary_password).map_err(|err| {
                NativeResponse::Error {
                    message: err.to_string(),
                }
            })?;

        (
            true,
            "[encrypted]".to_string(),
            Some(entry_key_wrapped),
            Some(entry_key_nonce),
            Some(entry_key_salt),
            Some(encrypted_secret),
            Some(encrypted_secret_nonce),
        )
    } else {
        (false, password.0.to_string(), None, None, None, None, None)
    };

    Ok(Entry {
        name,
        secret,
        secret_type: SecretType::Password,
        network: "Password".to_string(),
        public_address: None,
        username,
        url,
        site_rules: Vec::new(),
        notes: String::new(),
        created_at: now,
        updated_at: now,
        has_secondary_password,
        entry_key_wrapped,
        entry_key_nonce,
        entry_key_salt,
        encrypted_secret,
        encrypted_secret_nonce,
    })
}

fn save_password_entry(
    state: &mut HostState,
    name: String,
    username: Option<String>,
    password: SensitiveString,
    url: Option<String>,
    secondary_password: Option<SensitiveString>,
) -> NativeResponse {
    if state.session.is_none() {
        return NativeResponse::Error {
            message: "Vault is locked. Unlock it first.".to_string(),
        };
    }

    let entry = match build_password_entry(name, username, password, url, secondary_password) {
        Ok(entry) => entry,
        Err(response) => return response,
    };

    let entry_name = entry.name.clone();
    let session = state
        .session
        .as_mut()
        .expect("retained session was checked before building the entry");
    let snapshot = session.vault.clone();
    if let Err(err) = session.vault.push_entry(entry) {
        return NativeResponse::Error {
            message: err.to_string(),
        };
    }
    let save_result = session.save();
    match save_result {
        Ok(()) => {
            state.issued_matches.clear();
            NativeResponse::SaveEntry { entry_name }
        }
        Err(err) => {
            session.vault = snapshot;
            let conflicted = matches!(err, termkey::error::TermKeyError::VaultConflict);
            if conflicted {
                state.session = None;
                state.issued_matches.clear();
            }
            NativeResponse::Error {
                message: err.to_string(),
            }
        }
    }
}

fn handle_request(state: &mut HostState, payload: &[u8]) -> NativeResponse {
    let request = match serde_json::from_slice::<NativeRequest>(payload) {
        Ok(request) => request,
        Err(err) => {
            return NativeResponse::Error {
                message: format!("invalid request: {err}"),
            }
        }
    };

    if let NativeRequest::Ping { protocol_version } | NativeRequest::Status { protocol_version } =
        &request
    {
        if *protocol_version != Some(NATIVE_PROTOCOL_VERSION) {
            state.protocol_negotiated = false;
            return NativeResponse::Error {
                message: protocol_repair_message().to_string(),
            };
        }
        state.protocol_negotiated = true;
    }

    if matches!(request, NativeRequest::Ping { .. }) {
        return NativeResponse::Pong {
            app: "termkey",
            version: env!("CARGO_PKG_VERSION"),
            protocol_version: NATIVE_PROTOCOL_VERSION,
            capabilities: NATIVE_CAPABILITIES,
        };
    }

    if !state.protocol_negotiated {
        return NativeResponse::Error {
            message: protocol_repair_message().to_string(),
        };
    }

    match request {
        NativeRequest::Ping { .. } => unreachable!("ping handled above"),
        NativeRequest::Status { .. } => NativeResponse::Status(load_status_for_state(state)),
        NativeRequest::GeneratePassword => NativeResponse::GeneratedPassword {
            password: crypto::passwords::generate_password(),
        },
        NativeRequest::GetAutofillEntry {
            id,
            origin,
            secondary_password,
        } => get_autofill_entry(state, id, origin, secondary_password),
        NativeRequest::FindSiteMatches { url } => find_site_matches(state, url),
        NativeRequest::SavePasswordEntry {
            name,
            username,
            password,
            url,
            secondary_password,
        } => save_password_entry(state, name, username, password, url, secondary_password),
        NativeRequest::ListEntries => list_entries(state),
        NativeRequest::Unlock { password } => unlock_vault(state, password),
    }
}

fn protocol_repair_message() -> &'static str {
    "TermKey browser integration is out of date. Run `termkey browser repair`."
}

fn protocol_info_json() -> serde_json::Value {
    serde_json::json!({
        "app": "termkey",
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersion": NATIVE_PROTOCOL_VERSION,
        "capabilities": NATIVE_CAPABILITIES,
    })
}

fn is_valid_request_id(request_id: &str) -> bool {
    request_id.len() == 64
        && request_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Deserialize)]
struct NativeRequestEnvelope<'a> {
    #[serde(rename = "requestId", borrow)]
    request_id: Option<&'a str>,
}

fn handle_wire_request(state: &mut HostState, payload: &[u8]) -> NativeWireResponse {
    let request_id = serde_json::from_slice::<NativeRequestEnvelope<'_>>(payload)
        .ok()
        .and_then(|envelope| envelope.request_id)
        .filter(|request_id| is_valid_request_id(request_id))
        .map(str::to_owned)
        .unwrap_or_default();

    if request_id.is_empty() {
        return NativeWireResponse {
            request_id,
            response: NativeResponse::Error {
                message: "invalid request: requestId must be 64 lowercase hexadecimal characters"
                    .to_string(),
            },
        };
    }

    NativeWireResponse {
        request_id,
        response: handle_request(state, payload),
    }
}

fn main() -> io::Result<()> {
    crypto::secure::harden_process();
    apply_configured_vault_dir_override();

    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--protocol-info")) {
        serde_json::to_writer(io::stdout().lock(), &protocol_info_json())
            .map_err(io::Error::other)?;
        return Ok(());
    }

    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut state = HostState::default();

    while let Some(payload) = read_message(&mut stdin)? {
        let mut response = handle_wire_request(&mut state, &payload);
        write_wire_message(&mut stdout, &mut response)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        encode_wire_response, entry_fingerprint, find_unique_entry_by_fingerprint, handle_request,
        handle_wire_request, load_status_for_state, protocol_info_json, read_message,
        write_message, write_wire_message, AutofillEntryResponse, HostState, NativeRequest,
        NativeResponse, NativeWireResponse, SensitiveString, MAX_ISSUED_MATCHES,
        MAX_NATIVE_RESPONSE_BYTES, NATIVE_CAPABILITIES, NATIVE_PROTOCOL_VERSION,
    };
    use chrono::Utc;
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    use tempfile::TempDir;
    use termkey::crypto::entry_key;
    use termkey::error::TermKeyError;
    use termkey::vault::model::{Entry, SecretType, VaultData};
    use termkey::vault::session::VaultSession;
    use termkey::vault::storage::{read_vault, write_vault};
    use zeroize::Zeroizing;

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn test_vault_with_entry() -> VaultData {
        VaultData {
            entries: vec![Entry {
                name: "Email".to_string(),
                secret: "super-secret".to_string(),
                secret_type: SecretType::Password,
                network: "Password".to_string(),
                public_address: None,
                username: Some("ryan".to_string()),
                url: Some("https://example.com".to_string()),
                site_rules: Vec::new(),
                notes: String::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                has_secondary_password: false,
                entry_key_wrapped: None,
                entry_key_nonce: None,
                entry_key_salt: None,
                encrypted_secret: None,
                encrypted_secret_nonce: None,
            }],
            version: 1,
            revision: 0,
        }
    }

    fn unlocked_state(path: &std::path::Path) -> HostState {
        HostState {
            session: Some(
                VaultSession::open(
                    Zeroizing::new("correct horse battery staple".to_string()),
                    path.to_path_buf(),
                )
                .unwrap()
                .session,
            ),
            issued_matches: HashMap::new(),
            protocol_negotiated: true,
        }
    }

    fn negotiated_state() -> HostState {
        HostState {
            protocol_negotiated: true,
            ..HostState::default()
        }
    }

    fn discover_first_match_id(state: &mut HostState, origin: &str) -> String {
        match handle_request(
            state,
            format!(r#"{{"type":"find_site_matches","url":"{origin}"}}"#).as_bytes(),
        ) {
            NativeResponse::SiteMatches(response) => response
                .matches
                .first()
                .expect("fixture should have an authorized site match")
                .id
                .clone(),
            other => panic!("unexpected discovery response: {other:?}"),
        }
    }

    fn test_vault_with_secondary_entry() -> VaultData {
        let entry_key = entry_key::generate_entry_key();
        let (encrypted_secret, encrypted_secret_nonce) =
            entry_key::encrypt_secret(&entry_key, "super-secret").unwrap();
        let (wrapped_key, key_nonce, key_salt) =
            entry_key::wrap_entry_key(&entry_key, "view-pass").unwrap();

        VaultData {
            entries: vec![Entry {
                name: "Protected Email".to_string(),
                secret: "[encrypted]".to_string(),
                secret_type: SecretType::Password,
                network: "Password".to_string(),
                public_address: None,
                username: Some("ryan".to_string()),
                url: Some("https://secure.example.com".to_string()),
                site_rules: Vec::new(),
                notes: String::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                has_secondary_password: true,
                entry_key_wrapped: Some(wrapped_key),
                entry_key_nonce: Some(key_nonce),
                entry_key_salt: Some(key_salt),
                encrypted_secret: Some(encrypted_secret),
                encrypted_secret_nonce: Some(encrypted_secret_nonce),
            }],
            version: 1,
            revision: 0,
        }
    }

    fn test_vault_with_domain_rule_entry() -> VaultData {
        VaultData {
            entries: vec![Entry {
                name: "Google Account".to_string(),
                secret: "super-secret".to_string(),
                secret_type: SecretType::Password,
                network: "Password".to_string(),
                public_address: None,
                username: Some("ryan".to_string()),
                url: Some("https://accounts.google.com".to_string()),
                site_rules: vec!["domain:google.com".to_string()],
                notes: String::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                has_secondary_password: false,
                entry_key_wrapped: None,
                entry_key_nonce: None,
                entry_key_salt: None,
                encrypted_secret: None,
                encrypted_secret_nonce: None,
            }],
            version: 1,
            revision: 0,
        }
    }

    fn test_vault_with_port_specific_entry() -> VaultData {
        VaultData {
            entries: vec![Entry {
                name: "Dashboard".to_string(),
                secret: "super-secret".to_string(),
                secret_type: SecretType::Password,
                network: "Password".to_string(),
                public_address: None,
                username: Some("admin".to_string()),
                url: Some("https://home.ryanonmars.space:3000".to_string()),
                site_rules: Vec::new(),
                notes: String::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                has_secondary_password: false,
                entry_key_wrapped: None,
                entry_key_nonce: None,
                entry_key_salt: None,
                encrypted_secret: None,
                encrypted_secret_nonce: None,
            }],
            version: 1,
            revision: 0,
        }
    }

    fn test_vault_with_explicit_site_rule_entry() -> VaultData {
        VaultData {
            entries: vec![Entry {
                name: "Admin Login".to_string(),
                secret: "super-secret".to_string(),
                secret_type: SecretType::Password,
                network: "Password".to_string(),
                public_address: None,
                username: Some("ryan".to_string()),
                url: None,
                site_rules: vec![
                    "host:auth.example.com".to_string(),
                    "domain:example.com".to_string(),
                ],
                notes: String::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                has_secondary_password: false,
                entry_key_wrapped: None,
                entry_key_nonce: None,
                entry_key_salt: None,
                encrypted_secret: None,
                encrypted_secret_nonce: None,
            }],
            version: 1,
            revision: 0,
        }
    }

    #[test]
    fn ping_requires_current_protocol_version() {
        let response = handle_request(&mut HostState::default(), br#"{"type":"ping"}"#);

        assert!(matches!(
            response,
            NativeResponse::Error { message } if message.contains("browser repair")
        ));
    }

    #[test]
    fn current_protocol_ping_returns_capabilities_and_unlocks_privileged_requests() {
        let mut state = negotiated_state();
        let response = handle_request(&mut state, br#"{"type":"ping","protocolVersion":2}"#);

        assert!(matches!(
            response,
            NativeResponse::Pong {
                protocol_version: 2,
                capabilities: NATIVE_CAPABILITIES,
                ..
            }
        ));
        assert!(state.protocol_negotiated);
    }

    #[test]
    fn protocol_info_mode_reports_actual_binary_compatibility() {
        let info = protocol_info_json();
        assert_eq!(info["app"], "termkey");
        assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(info["protocolVersion"], NATIVE_PROTOCOL_VERSION);
        for capability in NATIVE_CAPABILITIES {
            assert!(info["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value.as_str() == Some(capability)));
        }
    }

    #[test]
    fn invalid_json_returns_error() {
        let response = handle_request(&mut HostState::default(), b"not-json");

        assert!(matches!(response, NativeResponse::Error { .. }));
    }

    #[test]
    fn generate_password_returns_generated_password() {
        let response = handle_request(&mut negotiated_state(), br#"{"type":"generate_password"}"#);

        match response {
            NativeResponse::GeneratedPassword { password } => {
                assert_eq!(password.len(), 24);
            }
            other => panic!("unexpected response: {:?}", other),
        }
    }

    #[test]
    fn status_returns_status_payload() {
        let mut state = HostState::default();
        let response = handle_request(&mut state, br#"{"type":"status","protocolVersion":2}"#);

        assert!(matches!(response, NativeResponse::Status(_)));
        assert!(state.protocol_negotiated);
    }

    #[test]
    fn status_is_unlocked_when_state_has_session() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(&VaultData::new(), b"correct horse battery staple", &path).unwrap();
        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());
        let mut state = negotiated_state();
        let response = handle_request(
            &mut state,
            br#"{"type":"unlock","password":"correct horse battery staple"}"#,
        );
        let status = load_status_for_state(&state);
        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        assert!(matches!(
            response,
            NativeResponse::Unlock { unlocked: true, .. }
        ));
        assert!(!status.locked);
    }

    #[test]
    fn unlock_succeeds_with_valid_password() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(&VaultData::new(), b"correct horse battery staple", &path).unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());
        let response = handle_request(
            &mut negotiated_state(),
            br#"{"type":"unlock","password":"correct horse battery staple"}"#,
        );
        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        assert!(matches!(
            response,
            NativeResponse::Unlock { unlocked: true, .. }
        ));
    }

    #[test]
    fn native_unlock_surfaces_post_migration_recovery_notice() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(&VaultData::new(), b"correct horse battery staple", &path).unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());
        let response = handle_request(
            &mut negotiated_state(),
            br#"{"type":"unlock","password":"correct horse battery staple"}"#,
        );
        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["unlocked"], true);
        assert!(
            json["recoveryNotice"]
                .as_str()
                .is_some_and(|notice| notice.to_ascii_lowercase().contains("recovery phrase")),
            "native unlock did not return a recovery notice: {json}"
        );
    }

    #[test]
    fn list_entries_requires_unlock() {
        let response = handle_request(&mut HostState::default(), br#"{"type":"list_entries"}"#);

        assert!(matches!(response, NativeResponse::Error { .. }));
    }

    #[test]
    fn list_entries_returns_metadata_when_unlocked() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());

        let mut state = HostState {
            session: Some(
                VaultSession::open(
                    Zeroizing::new("correct horse battery staple".to_string()),
                    path.clone(),
                )
                .unwrap()
                .session,
            ),
            issued_matches: HashMap::new(),
            protocol_negotiated: true,
        };
        let response = handle_request(&mut state, br#"{"type":"list_entries"}"#);

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        match response {
            NativeResponse::ListEntries { entries } => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].name, "Email");
                assert_eq!(entries[0].username.as_deref(), Some("ryan"));
            }
            other => panic!("unexpected response: {:?}", other),
        }
    }

    #[test]
    fn site_matches_require_unlock() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());

        let response = handle_request(
            &mut negotiated_state(),
            br#"{"type":"find_site_matches","url":"https://example.com"}"#,
        );

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        assert!(matches!(response, NativeResponse::Error { .. }));
    }

    #[test]
    fn autofill_rejects_invalid_requests() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();
        let mut state = unlocked_state(&path);

        for request in [
            br#"{"type":"get_autofill_entry","id":"1"}"#.as_slice(),
            br#"{"type":"get_autofill_entry","id":"1","origin":"http://example.com"}"#,
            br#"{"type":"get_autofill_entry","id":"1","origin":"https://user@example.com"}"#,
            br#"{"type":"get_autofill_entry","id":"1","origin":"https://example.com/login"}"#,
        ] {
            assert!(matches!(
                handle_request(&mut state, request),
                NativeResponse::Error { .. }
            ));
        }
    }

    #[test]
    fn autofill_rejects_entry_not_authorized_for_origin() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();

        let response = handle_request(
            &mut unlocked_state(&path),
            br#"{"type":"get_autofill_entry","id":"1","origin":"https://attacker.example"}"#,
        );

        assert!(matches!(response, NativeResponse::Error { .. }));
    }

    #[test]
    fn autofill_old_password_field_cannot_bypass_unlock() {
        let response = handle_request(
            &mut negotiated_state(),
            br#"{"type":"get_autofill_entry","id":"1","origin":"https://example.com","password":"correct horse battery staple"}"#,
        );

        assert!(matches!(
            response,
            NativeResponse::Error { message } if message.contains("locked")
        ));
    }

    #[test]
    fn save_old_master_password_field_cannot_bypass_unlock() {
        let response = handle_request(
            &mut negotiated_state(),
            br#"{"type":"save_password_entry","name":"Rejected","password":"secret","masterPassword":"correct horse battery staple"}"#,
        );

        assert!(matches!(
            response,
            NativeResponse::Error { message } if message.contains("locked")
        ));
    }

    #[test]
    fn autofill_rejects_an_entry_changed_after_discovery() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();
        let mut state = unlocked_state(&path);
        let handle = discover_first_match_id(&mut state, "https://example.com");
        let mut independent = VaultSession::open(
            Zeroizing::new("correct horse battery staple".into()),
            path.clone(),
        )
        .unwrap()
        .session;
        independent.vault.entries[0].secret = "changed-secret".into();
        independent.save().unwrap();

        let response = handle_request(
            &mut state,
            format!(
                r#"{{"type":"get_autofill_entry","id":"{handle}","origin":"https://example.com"}}"#
            )
            .as_bytes(),
        );

        assert!(matches!(response, NativeResponse::Error { .. }));
    }

    #[test]
    fn autofill_never_returns_the_entry_that_moves_into_a_deleted_match_position() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let mut original = test_vault_with_entry();
        let mut second = original.entries[0].clone();
        second.name = "Other".into();
        second.secret = "other-secret".into();
        original.entries.push(second);
        write_vault(&original, b"correct horse battery staple", &path).unwrap();
        let mut state = unlocked_state(&path);
        let handle = discover_first_match_id(&mut state, "https://example.com");

        let mut independent = VaultSession::open(
            Zeroizing::new("correct horse battery staple".into()),
            path.clone(),
        )
        .unwrap()
        .session;
        independent.vault.entries.remove(0);
        independent.save().unwrap();

        let response = handle_request(
            &mut state,
            format!(
                r#"{{"type":"get_autofill_entry","id":"{handle}","origin":"https://example.com"}}"#
            )
            .as_bytes(),
        );

        assert!(
            matches!(response, NativeResponse::Error { .. }),
            "deleted match handle returned another entry: {response:?}"
        );
    }

    #[test]
    fn discovery_issues_opaque_non_positional_handles_that_expire() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();
        let mut state = unlocked_state(&path);
        let handle = discover_first_match_id(&mut state, "https://example.com");

        assert_ne!(handle, "1");
        assert_eq!(handle.len(), 64);
        assert!(handle.bytes().all(|byte| byte.is_ascii_hexdigit()));
        state.issued_matches.get_mut(&handle).unwrap().expires_at =
            Instant::now() - Duration::from_millis(1);

        let response = handle_request(
            &mut state,
            format!(
                r#"{{"type":"get_autofill_entry","id":"{handle}","origin":"https://example.com"}}"#
            )
            .as_bytes(),
        );
        assert!(matches!(response, NativeResponse::Error { .. }));
    }

    #[test]
    fn discovery_capacity_never_returns_an_already_evicted_handle() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let mut vault = test_vault_with_entry();
        let template = vault.entries[0].clone();
        for index in 1..=100 {
            let mut entry = template.clone();
            entry.name = format!("Entry {index:03}");
            entry.username = Some(format!("user-{index}@example.com"));
            vault.entries.push(entry);
        }
        for entry in &mut vault.entries {
            entry.site_rules = vec!["domain:example.com".to_string()];
        }
        write_vault(&vault, b"correct horse battery staple", &path).unwrap();
        let mut state = unlocked_state(&path);

        let matches = match handle_request(
            &mut state,
            br#"{"type":"find_site_matches","url":"https://example.com"}"#,
        ) {
            NativeResponse::SiteMatches(response) => response.matches,
            other => panic!("unexpected response: {other:?}"),
        };

        assert_eq!(matches.len(), MAX_ISSUED_MATCHES);
        assert!(matches
            .iter()
            .all(|site_match| state.issued_matches.contains_key(&site_match.id)));

        for origin in ["https://example.com", "https://login.example.com"] {
            let refreshed = match handle_request(
                &mut state,
                format!(r#"{{"type":"find_site_matches","url":"{origin}"}}"#).as_bytes(),
            ) {
                NativeResponse::SiteMatches(response) => response.matches,
                other => panic!("unexpected response: {other:?}"),
            };
            assert_eq!(refreshed.len(), MAX_ISSUED_MATCHES);
            assert!(refreshed
                .iter()
                .all(|site_match| state.issued_matches.contains_key(&site_match.id)));
        }
    }

    #[test]
    fn duplicate_exact_fingerprints_fail_closed() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();
        let mut state = unlocked_state(&path);
        let duplicate = state.session.as_ref().unwrap().vault.entries[0].clone();
        state
            .session
            .as_mut()
            .unwrap()
            .vault
            .entries
            .push(duplicate);
        let fingerprint = entry_fingerprint(&state.session.as_ref().unwrap().vault.entries[0]);
        assert!(find_unique_entry_by_fingerprint(
            &state.session.as_ref().unwrap().vault,
            &fingerprint
        )
        .is_err());
    }

    #[test]
    fn autofill_reloads_authorization_before_release() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();
        let mut state = unlocked_state(&path);
        let handle = discover_first_match_id(&mut state, "https://example.com");
        let mut independent = VaultSession::open(
            Zeroizing::new("correct horse battery staple".into()),
            path.clone(),
        )
        .unwrap()
        .session;
        independent.vault.entries[0].url = Some("https://other.example".into());
        independent.save().unwrap();

        let response = handle_request(
            &mut state,
            format!(
                r#"{{"type":"get_autofill_entry","id":"{handle}","origin":"https://example.com"}}"#
            )
            .as_bytes(),
        );

        assert!(matches!(response, NativeResponse::Error { .. }));
    }

    #[test]
    fn autofill_replacement_vault_locks_session() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();
        let mut state = unlocked_state(&path);
        let handle = discover_first_match_id(&mut state, "https://example.com");
        let replacement = termkey::vault::format::encode_v3(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            termkey::vault::model::VaultHeader::MAGIC,
            None,
            None,
        )
        .unwrap();
        std::fs::write(path, replacement.as_slice()).unwrap();

        let response = handle_request(
            &mut state,
            format!(
                r#"{{"type":"get_autofill_entry","id":"{handle}","origin":"https://example.com"}}"#
            )
            .as_bytes(),
        );

        assert!(matches!(response, NativeResponse::Error { .. }));
        assert!(state.session.is_none());
    }

    #[test]
    fn site_matches_support_registrable_domain_matching() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_domain_rule_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());

        let response = handle_request(
            &mut unlocked_state(&path),
            br#"{"type":"find_site_matches","url":"https://mail.google.com"}"#,
        );

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        match response {
            NativeResponse::SiteMatches(matches) => {
                assert_eq!(matches.matches.len(), 1);
                assert_eq!(matches.matches[0].match_type, "registrable_domain");
            }
            other => panic!("unexpected response: {:?}", other),
        }
    }

    #[test]
    fn site_matches_support_http_origins_but_reject_page_urls() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();

        let http = handle_request(
            &mut unlocked_state(&path),
            br#"{"type":"find_site_matches","url":"http://example.com"}"#,
        );
        let page_url = handle_request(
            &mut unlocked_state(&path),
            br#"{"type":"find_site_matches","url":"https://example.com/login"}"#,
        );

        match http {
            NativeResponse::SiteMatches(matches) => assert!(matches.matches.is_empty()),
            other => panic!("unexpected response: {:?}", other),
        }
        assert!(matches!(page_url, NativeResponse::Error { .. }));
    }

    #[test]
    fn site_matches_support_explicit_site_rules_without_url() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_explicit_site_rule_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());

        let response = handle_request(
            &mut unlocked_state(&path),
            br#"{"type":"find_site_matches","url":"https://dashboard.example.com"}"#,
        );

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        match response {
            NativeResponse::SiteMatches(matches) => {
                assert_eq!(matches.matches.len(), 1);
                assert_eq!(matches.matches[0].name, "Admin Login");
            }
            other => panic!("unexpected response: {:?}", other),
        }
    }

    #[test]
    fn site_matches_do_not_cross_non_default_ports_by_default() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_port_specific_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());

        let response = handle_request(
            &mut unlocked_state(&path),
            br#"{"type":"find_site_matches","url":"https://home.ryanonmars.space"}"#,
        );

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        match response {
            NativeResponse::SiteMatches(matches) => {
                assert!(matches.matches.is_empty());
            }
            other => panic!("unexpected response: {:?}", other),
        }
    }

    #[test]
    fn save_password_entry_persists_with_retained_unlock() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(&VaultData::new(), b"correct horse battery staple", &path).unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());

        let response = handle_request(
            &mut unlocked_state(&path),
            br#"{"type":"save_password_entry","name":"Example Login","username":"ryan@example.com","password":"super-secret","url":"https://EXAMPLE.com:443/"}"#,
        );

        let saved_vault = read_vault(b"correct horse battery staple", &path).unwrap();

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        match response {
            NativeResponse::SaveEntry { entry_name } => {
                assert_eq!(entry_name, "Example Login");
            }
            other => panic!("unexpected response: {:?}", other),
        }

        assert_eq!(saved_vault.entries.len(), 1);
        assert_eq!(saved_vault.entries[0].name, "Example Login");
        assert_eq!(
            saved_vault.entries[0].username.as_deref(),
            Some("ryan@example.com")
        );
        assert_eq!(
            saved_vault.entries[0].url.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(saved_vault.entries[0].secret, "super-secret");
        assert!(!saved_vault.entries[0].has_secondary_password);
    }

    #[test]
    fn native_save_rejects_non_origin_urls_before_mutating_the_vault() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(&VaultData::new(), b"correct horse battery staple", &path).unwrap();
        let mut state = unlocked_state(&path);

        for (index, url) in [
            "https://user@example.com",
            "https://user:password@example.com",
            "https://example.com/login",
            "https://example.com?next=home",
            "https://example.com#fragment",
            " https://example.com",
        ]
        .into_iter()
        .enumerate()
        {
            let response = handle_request(
                &mut state,
                serde_json::json!({
                    "type": "save_password_entry",
                    "name": format!("Rejected {index}"),
                    "password": "secret",
                    "url": url,
                })
                .to_string()
                .as_bytes(),
            );
            assert!(
                matches!(response, NativeResponse::Error { .. }),
                "native save accepted {url:?}"
            );
        }
        assert!(state.session.as_ref().unwrap().vault.entries.is_empty());
    }

    #[test]
    fn native_save_persists_canonical_http_origin() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(&VaultData::new(), b"correct horse battery staple", &path).unwrap();

        let response = handle_request(
            &mut unlocked_state(&path),
            br#"{"type":"save_password_entry","name":"qBittorrent","password":"secret","url":"http://192.168.4.64:8080/"}"#,
        );

        assert!(matches!(response, NativeResponse::SaveEntry { .. }));
        let vault = read_vault(b"correct horse battery staple", &path).unwrap();
        assert_eq!(
            vault.entries[0].url.as_deref(),
            Some("http://192.168.4.64:8080")
        );
    }

    #[test]
    fn native_host_discovers_and_fills_http_origin_exactly() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let mut vault = test_vault_with_entry();
        vault.entries[0].url = Some("http://192.168.4.64:8080".into());
        write_vault(&vault, b"correct horse battery staple", &path).unwrap();
        let mut state = unlocked_state(&path);

        let id = discover_first_match_id(&mut state, "http://192.168.4.64:8080");
        let response = handle_request(
            &mut state,
            format!(
                r#"{{"type":"get_autofill_entry","id":"{id}","origin":"http://192.168.4.64:8080"}}"#
            )
            .as_bytes(),
        );

        assert!(matches!(response, NativeResponse::AutofillEntry { .. }));
    }

    #[test]
    fn save_password_entry_supports_secondary_password() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(&VaultData::new(), b"correct horse battery staple", &path).unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());

        let mut state = unlocked_state(&path);
        let save_response = handle_request(
            &mut state,
            br#"{"type":"save_password_entry","name":"Protected Login","username":"ryan@example.com","password":"super-secret","url":"https://secure.example.com","secondaryPassword":"view-pass"}"#,
        );
        let handle = discover_first_match_id(&mut state, "https://secure.example.com");
        let autofill_response = handle_request(
            &mut state,
            format!(
                r#"{{"type":"get_autofill_entry","id":"{handle}","origin":"https://secure.example.com","secondaryPassword":"view-pass"}}"#
            )
            .as_bytes(),
        );

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        assert!(matches!(save_response, NativeResponse::SaveEntry { .. }));

        match autofill_response {
            NativeResponse::AutofillEntry { entry } => {
                assert_eq!(entry.name, "Protected Login");
                assert_eq!(entry.password, "super-secret");
            }
            other => panic!("unexpected response: {:?}", other),
        }
    }

    #[test]
    fn save_password_entry_duplicate_preserves_existing_protected_entry_in_memory_and_on_disk() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_secondary_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());
        let mut state = unlocked_state(&path);
        let original_memory = serde_json::to_vec(&state.session.as_ref().unwrap().vault).unwrap();
        let original_disk = std::fs::read(&path).unwrap();

        let response = handle_request(
            &mut state,
            br#"{"type":"save_password_entry","name":"protected email","username":"attacker","password":"replacement","url":"https://evil.example"}"#,
        );

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        assert!(matches!(
            response,
            NativeResponse::Error { message }
                if message.contains("already exists")
        ));
        assert_eq!(
            serde_json::to_vec(&state.session.as_ref().unwrap().vault).unwrap(),
            original_memory
        );
        assert_eq!(std::fs::read(&path).unwrap(), original_disk);

        let persisted = read_vault(b"correct horse battery staple", &path).unwrap();
        assert_eq!(serde_json::to_vec(&persisted).unwrap(), original_memory);
        let existing = persisted.find_entry("PROTECTED EMAIL").unwrap();
        assert_eq!(
            &*existing.reveal_secret(Some("view-pass")).unwrap(),
            "super-secret"
        );
        assert_eq!(existing.username.as_deref(), Some("ryan"));
        assert_eq!(existing.url.as_deref(), Some("https://secure.example.com"));
    }

    #[test]
    fn failed_retained_save_rolls_back_unsaved_entry() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();
        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());
        let mut state = negotiated_state();
        assert!(matches!(
            handle_request(
                &mut state,
                br#"{"type":"unlock","password":"correct horse battery staple"}"#,
            ),
            NativeResponse::Unlock { unlocked: true, .. }
        ));
        std::fs::remove_file(&path).unwrap();

        let save_response = handle_request(
            &mut state,
            br#"{"type":"save_password_entry","name":"Unsaved","password":"secret"}"#,
        );
        let list_response = handle_request(&mut state, br#"{"type":"list_entries"}"#);
        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        assert!(matches!(save_response, NativeResponse::Error { .. }));
        match list_response {
            NativeResponse::ListEntries { entries } => {
                assert!(entries.iter().all(|entry| entry.name != "Unsaved"));
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn retained_conflict_invalidates_session_without_exposing_unsaved_entry() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();
        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());
        let mut state = unlocked_state(&path);
        let mut independent = VaultSession::open(
            Zeroizing::new("correct horse battery staple".into()),
            path.clone(),
        )
        .unwrap()
        .session;
        independent.vault.entries[0].notes = "external update".into();
        independent.save().unwrap();

        let save_response = handle_request(
            &mut state,
            br#"{"type":"save_password_entry","name":"Unsaved","password":"secret"}"#,
        );
        let list_response = handle_request(&mut state, br#"{"type":"list_entries"}"#);
        let persisted = read_vault(b"correct horse battery staple", &path).unwrap();
        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        assert!(matches!(save_response, NativeResponse::Error { .. }));
        assert!(matches!(list_response, NativeResponse::Error { .. }));
        assert!(persisted
            .entries
            .iter()
            .all(|entry| entry.name != "Unsaved"));
        assert_eq!(persisted.entries[0].notes, "external update");
    }

    #[test]
    fn autofill_secondary_password_entry_requires_secondary_password() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_secondary_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());

        let mut state = unlocked_state(&path);
        let handle = discover_first_match_id(&mut state, "https://secure.example.com");
        let response = handle_request(
            &mut state,
            format!(
                r#"{{"type":"get_autofill_entry","id":"{handle}","origin":"https://secure.example.com"}}"#
            )
            .as_bytes(),
        );

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        match response {
            NativeResponse::Error { message } => {
                assert_eq!(message, "This entry requires a secondary password to view.");
            }
            other => panic!("unexpected response: {:?}", other),
        }
    }

    #[test]
    fn autofill_secondary_password_entry_accepts_secondary_password() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_secondary_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());

        let mut state = unlocked_state(&path);
        let handle = discover_first_match_id(&mut state, "https://secure.example.com");
        let response = handle_request(
            &mut state,
            format!(
                r#"{{"type":"get_autofill_entry","id":"{handle}","origin":"https://secure.example.com","secondary_password":"view-pass"}}"#
            )
            .as_bytes(),
        );

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        match response {
            NativeResponse::AutofillEntry { entry } => {
                assert_eq!(entry.name, "Protected Email");
                assert_eq!(entry.password, "super-secret");
            }
            other => panic!("unexpected response: {:?}", other),
        }
    }

    #[test]
    fn autofill_secondary_password_entry_accepts_camel_case_secondary_password() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_secondary_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();

        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());

        let mut state = unlocked_state(&path);
        let handle = discover_first_match_id(&mut state, "https://secure.example.com");
        let response = handle_request(
            &mut state,
            format!(
                r#"{{"type":"get_autofill_entry","id":"{handle}","origin":"https://secure.example.com","secondaryPassword":"view-pass"}}"#
            )
            .as_bytes(),
        );

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }

        match response {
            NativeResponse::AutofillEntry { entry } => {
                assert_eq!(entry.name, "Protected Email");
                assert_eq!(entry.password, "super-secret");
            }
            other => panic!("unexpected response: {:?}", other),
        }
    }

    #[test]
    fn autofill_secondary_password_entry_rejects_wrong_secondary_password() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(
            &test_vault_with_secondary_entry(),
            b"correct horse battery staple",
            &path,
        )
        .unwrap();
        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());

        let mut state = unlocked_state(&path);
        let handle = discover_first_match_id(&mut state, "https://secure.example.com");
        let response = handle_request(
            &mut state,
            format!(
                r#"{{"type":"get_autofill_entry","id":"{handle}","origin":"https://secure.example.com","secondaryPassword":"wrong-pass"}}"#
            )
            .as_bytes(),
        );

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }
        assert!(matches!(
            response,
            NativeResponse::Error { message }
                if message.contains("Incorrect secondary password")
        ));
    }

    #[test]
    fn malformed_marker_backed_entry_is_rejected_before_persistence() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        let mut malformed = test_vault_with_secondary_entry();
        malformed.entries[0].encrypted_secret_nonce = None;

        assert!(matches!(
            write_vault(&malformed, b"correct horse battery staple", &path),
            Err(TermKeyError::InvalidEntry(_))
        ));
        assert!(
            !path.exists(),
            "invalid vault data reached the persistence boundary"
        );
    }

    #[test]
    fn message_roundtrip_uses_native_framing() {
        let mut out = Vec::new();
        write_message(
            &mut out,
            &NativeResponse::Pong {
                app: "termkey",
                version: env!("CARGO_PKG_VERSION"),
                protocol_version: NATIVE_PROTOCOL_VERSION,
                capabilities: NATIVE_CAPABILITIES,
            },
        )
        .unwrap();

        let payload = read_message(&mut Cursor::new(out)).unwrap().unwrap();
        let decoded: serde_json::Value = serde_json::from_slice(&payload).unwrap();

        assert_eq!(decoded["type"], "pong");
        assert_eq!(decoded["app"], "termkey");
    }

    #[test]
    fn native_handshake_uses_camel_case_protocol_version() {
        let request_id = "a".repeat(64);
        let response = handle_wire_request(
            &mut HostState::default(),
            format!(r#"{{"type":"ping","protocolVersion":2,"requestId":"{request_id}"}}"#)
                .as_bytes(),
        );

        let encoded = serde_json::to_value(response).unwrap();

        assert_eq!(encoded["type"], "pong");
        assert_eq!(encoded["protocolVersion"], NATIVE_PROTOCOL_VERSION);
        assert!(encoded.get("protocol_version").is_none());
    }

    #[test]
    fn native_message_rejects_payload_over_64_mib_before_allocation() {
        let oversized = (64_u32 * 1024 * 1024) + 1;
        let error = read_message(&mut Cursor::new(oversized.to_le_bytes())).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn wire_success_echoes_valid_request_id() {
        let request_id = "a".repeat(64);
        let response = handle_wire_request(
            &mut HostState::default(),
            format!(r#"{{"type":"ping","protocolVersion":2,"requestId":"{request_id}"}}"#)
                .as_bytes(),
        );
        let encoded = serde_json::to_value(response).unwrap();

        assert_eq!(encoded["requestId"], request_id);
        assert_eq!(encoded["type"], "pong");
    }

    #[test]
    fn wire_application_error_echoes_valid_request_id() {
        let request_id = "b".repeat(64);
        let response = handle_wire_request(
            &mut negotiated_state(),
            format!(
                r#"{{"type":"get_autofill_entry","requestId":"{request_id}","id":"1","origin":"https://example.com"}}"#
            )
            .as_bytes(),
        );
        let encoded = serde_json::to_value(response).unwrap();

        assert_eq!(encoded["requestId"], request_id);
        assert_eq!(encoded["type"], "error");
        assert!(encoded["message"].as_str().unwrap().contains("locked"));
    }

    #[test]
    fn wire_rejects_missing_and_malformed_request_ids() {
        let missing = serde_json::to_value(handle_wire_request(
            &mut HostState::default(),
            br#"{"type":"ping"}"#,
        ))
        .unwrap();
        let malformed = serde_json::to_value(handle_wire_request(
            &mut HostState::default(),
            br#"{"type":"ping","requestId":"short"}"#,
        ))
        .unwrap();

        assert_eq!(missing["requestId"], "");
        assert_eq!(missing["type"], "error");
        assert!(missing["message"].as_str().unwrap().contains("requestId"));
        assert_eq!(malformed["requestId"], "");
        assert_eq!(malformed["type"], "error");
        assert!(malformed["message"].as_str().unwrap().contains("requestId"));
    }

    #[test]
    fn wire_never_echoes_an_oversized_invalid_request_id() {
        let oversized = "a".repeat(1_000_000);
        let response = handle_wire_request(
            &mut HostState::default(),
            serde_json::json!({ "type": "ping", "requestId": oversized })
                .to_string()
                .as_bytes(),
        );
        let encoded = serde_json::to_vec(&response).unwrap();

        assert!(encoded.len() < 1024);
        assert_eq!(response.request_id, "");
    }

    #[test]
    fn wire_rejects_escaped_request_ids_without_owned_envelope_decoding() {
        let escaped = format!(
            r#"{{"type":"ping","protocolVersion":2,"requestId":"{}\u0061"}}"#,
            "a".repeat(63)
        );
        let response = handle_wire_request(&mut HostState::default(), escaped.as_bytes());

        assert_eq!(response.request_id, "");
        assert!(matches!(response.response, NativeResponse::Error { .. }));
    }

    #[test]
    fn outbound_wire_limit_allows_exact_boundary_and_compacts_beyond_it() {
        let request_id = "a".repeat(64);
        let base = NativeWireResponse {
            request_id: request_id.clone(),
            response: NativeResponse::GeneratedPassword {
                password: String::new(),
            },
        };
        let base_len = serde_json::to_vec(&base).unwrap().len();
        let exact = NativeWireResponse {
            request_id: request_id.clone(),
            response: NativeResponse::GeneratedPassword {
                password: "x".repeat(MAX_NATIVE_RESPONSE_BYTES - base_len),
            },
        };
        let beyond = NativeWireResponse {
            request_id: request_id.clone(),
            response: NativeResponse::GeneratedPassword {
                password: "x".repeat(MAX_NATIVE_RESPONSE_BYTES - base_len + 1),
            },
        };

        let exact_encoded = encode_wire_response(&exact).unwrap();
        let beyond_encoded = encode_wire_response(&beyond).unwrap();
        let beyond_json: serde_json::Value = serde_json::from_slice(&beyond_encoded).unwrap();

        assert_eq!(exact_encoded.len(), MAX_NATIVE_RESPONSE_BYTES);
        assert_eq!(exact_encoded.capacity(), MAX_NATIVE_RESPONSE_BYTES);
        assert!(beyond_encoded.len() < MAX_NATIVE_RESPONSE_BYTES);
        assert_eq!(beyond_json["requestId"], request_id);
        assert_eq!(beyond_json["type"], "error");
    }

    #[test]
    fn writing_wire_responses_clears_transient_generated_and_autofill_secrets() {
        let request_id = "a".repeat(64);
        for response in [
            NativeResponse::GeneratedPassword {
                password: "generated-secret".to_string(),
            },
            NativeResponse::AutofillEntry {
                entry: AutofillEntryResponse {
                    id: "b".repeat(64),
                    name: "Example".to_string(),
                    username: Some("person@example.test".to_string()),
                    password: "autofill-secret".to_string(),
                },
            },
        ] {
            let mut wire = NativeWireResponse {
                request_id: request_id.clone(),
                response,
            };
            let mut framed = Vec::new();
            write_wire_message(&mut framed, &mut wire).unwrap();

            match &wire.response {
                NativeResponse::GeneratedPassword { password } => {
                    assert!(password.is_empty());
                }
                NativeResponse::AutofillEntry { entry } => {
                    assert!(entry.password.is_empty());
                }
                other => panic!("unexpected fixture response: {other:?}"),
            }
            let payload = read_message(&mut Cursor::new(framed)).unwrap().unwrap();
            fn assert_zeroizing_payload(_: &Zeroizing<Vec<u8>>) {}
            assert_zeroizing_payload(&payload);
            let serialized: serde_json::Value = serde_json::from_slice(&payload).unwrap();
            assert!(
                serialized["password"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
                    || serialized["entry"]["password"]
                        .as_str()
                        .is_some_and(|value| !value.is_empty())
            );
        }
    }

    #[test]
    fn sensitive_native_request_fields_deserialize_directly_into_zeroizing_storage() {
        let request: NativeRequest = serde_json::from_slice(
            br#"{"type":"save_password_entry","name":"Example","password":"save-secret","secondaryPassword":"view-secret"}"#,
        )
        .unwrap();

        let NativeRequest::SavePasswordEntry {
            password,
            secondary_password: Some(secondary_password),
            ..
        } = request
        else {
            panic!("unexpected parsed request");
        };
        fn assert_sensitive_wrapper(value: &SensitiveString, expected: &str) {
            let _: &Zeroizing<String> = &value.0;
            assert_eq!(value.0.as_str(), expected);
        }
        assert_sensitive_wrapper(&password, "save-secret");
        assert_sensitive_wrapper(&secondary_password, "view-secret");
    }

    #[test]
    fn sequential_wire_requests_retain_unlocked_host_state() {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.ck");
        write_vault(&VaultData::new(), b"correct horse battery staple", &path).unwrap();
        let previous_vault_dir = std::env::var_os("TERMKEY_VAULT_DIR");
        std::env::set_var("TERMKEY_VAULT_DIR", dir.path());
        let mut state = negotiated_state();
        let unlock_id = "c".repeat(64);
        let status_id = "d".repeat(64);

        let unlock = serde_json::to_value(handle_wire_request(
            &mut state,
            format!(
                r#"{{"type":"unlock","requestId":"{unlock_id}","password":"correct horse battery staple"}}"#
            )
            .as_bytes(),
        ))
        .unwrap();
        let status = serde_json::to_value(handle_wire_request(
            &mut state,
            format!(r#"{{"type":"status","protocolVersion":2,"requestId":"{status_id}"}}"#)
                .as_bytes(),
        ))
        .unwrap();

        match previous_vault_dir {
            Some(value) => std::env::set_var("TERMKEY_VAULT_DIR", value),
            None => std::env::remove_var("TERMKEY_VAULT_DIR"),
        }
        assert_eq!(unlock["requestId"], unlock_id);
        assert_eq!(unlock["type"], "unlock");
        assert_eq!(status["requestId"], status_id);
        assert_eq!(status["type"], "status");
        assert_eq!(status["locked"], false);
    }
}
