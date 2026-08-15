use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::cli::BrowserCommands;
use crate::error::{Result, TermKeyError};
use crate::native::site::{NATIVE_CAPABILITIES, NATIVE_PROTOCOL_VERSION};
use crate::ui::borders::print_success;

const CHROME_EXTENSION_ID: &str = "dancadidkgcdlfdlfpbmmiokkeedpini";
const CHROME_WEB_STORE_URL: &str =
    "https://chromewebstore.google.com/detail/dancadidkgcdlfdlfpbmmiokkeedpini";
const CHROME_NATIVE_HOST_NAME: &str = "com.ryanonmars.termkey";
const NATIVE_HOST_BINARY_ENV: &str = "TERMKEY_NATIVE_HOST_BINARY";

pub fn run(command: &BrowserCommands) -> Result<()> {
    match command {
        BrowserCommands::Install => install_browser_support("installed"),
        BrowserCommands::Repair => install_browser_support("reinstalled"),
        BrowserCommands::Status => print_status(),
        BrowserCommands::Uninstall => {
            let report = uninstall_browser_support()?;
            if report.removed_native_host_manifest {
                print_success("TermKey native messaging manifest removed.");
            } else {
                println!("  TermKey native messaging manifest is already removed.");
            }
            println!("  The Chrome Web Store extension remains installed and managed by Chrome.");
            println!("  Remove it from chrome://extensions if desired.");
            Ok(())
        }
    }
}

#[derive(Default)]
pub(crate) struct BrowserUninstallReport {
    pub removed_native_host_manifest: bool,
}

pub(crate) fn uninstall_browser_support() -> Result<BrowserUninstallReport> {
    remove_browser_support_at(&native_host_manifest_path()?)
}

fn remove_browser_support_at(native_host_manifest: &Path) -> Result<BrowserUninstallReport> {
    let mut report = BrowserUninstallReport::default();
    if native_host_manifest.exists() {
        fs::remove_file(native_host_manifest)?;
        report.removed_native_host_manifest = true;
    }
    Ok(report)
}

fn install_browser_support(action: &str) -> Result<()> {
    let native_host_binary = locate_native_host_binary()?;
    let manifest_path = install_native_host_manifest(&native_host_binary)?;
    let manifest_status = native_host_manifest_status(&manifest_path, Some(&native_host_binary))?;
    if manifest_status != "ready" {
        return Err(TermKeyError::ConfigError(format!(
            "Chrome native host manifest verification failed: {manifest_status}"
        )));
    }
    let protocol_status = native_host_protocol_status(&native_host_binary);
    if !protocol_status.starts_with("ready") {
        return Err(TermKeyError::ConfigError(format!(
            "Chrome native host protocol verification failed: {protocol_status}"
        )));
    }

    print_success(&format!("Chrome native messaging host {}.", action));
    println!();
    println!("  Native host manifest: {}", manifest_path.display());
    println!("  Native host protocol: {}", protocol_status);
    println!("  Expected Store extension ID: {}", CHROME_EXTENSION_ID);
    println!("  Chrome Web Store: {}", CHROME_WEB_STORE_URL);
    println!();
    if let Err(error) = webbrowser::open(CHROME_WEB_STORE_URL) {
        println!("  Could not open the Store listing automatically: {error}");
    }
    println!("  Install or enable TermKey Extension in Chrome, then retry the extension.");
    println!();
    println!("  Run `termkey browser status` to verify the native host setup.");

    Ok(())
}

fn print_status() -> Result<()> {
    let native_host_binary = locate_native_host_binary().ok();
    let manifest_path = native_host_manifest_path()?;
    let manifest_status =
        native_host_manifest_status(&manifest_path, native_host_binary.as_deref())?;
    let protocol_status = native_host_binary
        .as_deref()
        .map(native_host_protocol_status)
        .unwrap_or_else(|| "missing; run `termkey browser repair`".to_string());

    println!();
    println!("  TermKey Browser Integration");
    println!("  ───────────────────────────");
    println!("  Expected Store extension ID: {}", CHROME_EXTENSION_ID);
    println!("  Chrome Web Store: {}", CHROME_WEB_STORE_URL);
    println!(
        "  Native host binary: {}",
        describe_optional_path(native_host_binary.as_deref())
    );
    println!("  Native host protocol: {}", protocol_status);
    println!(
        "  Chrome native host manifest: {} ({})",
        manifest_path.display(),
        manifest_status
    );
    println!();
    println!("  Chrome owns the Store extension's install and enabled state.");
    println!("  Open the Store link above to install or manage it.");
    println!();
    println!(
        "  Use `termkey browser repair` if any path or protocol metadata is missing or stale."
    );

    Ok(())
}

fn describe_optional_path(path: Option<&Path>) -> String {
    match path {
        Some(path) => describe_existing_path(path),
        None => "missing".to_string(),
    }
}

fn describe_existing_path(path: &Path) -> String {
    if path.exists() {
        format!("{} (present)", path.display())
    } else {
        format!("{} (missing)", path.display())
    }
}

fn locate_native_host_binary() -> Result<PathBuf> {
    let current_exe = std::env::current_exe().map_err(TermKeyError::Io)?;
    for candidate in native_host_binary_candidates(&current_exe) {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(TermKeyError::ConfigError(
        "Native host binary not found. Reinstall TermKey or build `termkey-native-host` first."
            .into(),
    ))
}

fn native_host_binary_candidates(current_exe: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let binary_name = native_host_binary_name();
    let executable_paths = executable_candidate_paths(current_exe);

    if let Ok(path) = std::env::var(NATIVE_HOST_BINARY_ENV) {
        candidates.push(PathBuf::from(path));
    }

    for executable_path in &executable_paths {
        if let Some(exe_dir) = executable_path.parent() {
            candidates.push(exe_dir.join(binary_name));

            for prefix in install_prefix_candidates(exe_dir) {
                candidates.push(prefix.join("libexec").join(binary_name));
                candidates.push(
                    prefix
                        .join("share")
                        .join("termkey")
                        .join("bin")
                        .join(binary_name),
                );
            }

            if let Some(repo_root) = repo_root_from_exe(exe_dir) {
                candidates.push(repo_root.join("target").join("debug").join(binary_name));
                candidates.push(repo_root.join("target").join("release").join(binary_name));
            }
        }

        if let Some(resources_dir) = macos_resources_dir(executable_path) {
            candidates.push(resources_dir.join("bin").join(binary_name));
        }
    }

    if let Some(resources_dir) = macos_installed_app_resources_dir() {
        candidates.push(resources_dir.join("bin").join(binary_name));
    }

    candidates
}

fn executable_candidate_paths(current_exe: &Path) -> Vec<PathBuf> {
    let mut paths = vec![current_exe.to_path_buf()];

    if let Ok(canonical) = fs::canonicalize(current_exe) {
        if canonical != current_exe {
            paths.push(canonical);
        }
    }

    paths
}

fn native_host_binary_name() -> &'static str {
    "termkey-native-host"
}

fn repo_root_from_exe(exe_dir: &Path) -> Option<PathBuf> {
    let target_dir = exe_dir.parent()?;
    if target_dir.file_name()?.to_str()? != "target" {
        return None;
    }

    Some(target_dir.parent()?.to_path_buf())
}

fn install_prefix_candidates(exe_dir: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(prefix) = exe_dir.parent() {
        candidates.push(prefix.to_path_buf());
    }

    candidates
}

fn macos_resources_dir(current_exe: &Path) -> Option<PathBuf> {
    let parent = current_exe.parent()?;

    if parent.file_name()?.to_str()? == "MacOS" {
        let contents_dir = parent.parent()?;
        if contents_dir.file_name()?.to_str()? != "Contents" {
            return None;
        }

        return Some(contents_dir.join("Resources"));
    }

    if parent.file_name()?.to_str()? == "bin" {
        let resources_dir = parent.parent()?;
        if resources_dir.file_name()?.to_str()? != "Resources" {
            return None;
        }

        let contents_dir = resources_dir.parent()?;
        if contents_dir.file_name()?.to_str()? != "Contents" {
            return None;
        }

        return Some(resources_dir.to_path_buf());
    }

    None
}

fn macos_installed_app_resources_dir() -> Option<PathBuf> {
    let path = PathBuf::from("/Applications/TermKey.app/Contents/Resources");
    if path.exists() {
        return Some(path);
    }

    None
}

fn install_native_host_manifest(native_host_binary: &Path) -> Result<PathBuf> {
    let manifest_path = native_host_manifest_path()?;

    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let manifest = serde_json::json!({
        "name": CHROME_NATIVE_HOST_NAME,
        "description": "TermKey native messaging host",
        "path": native_host_binary.display().to_string(),
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{}/", CHROME_EXTENSION_ID)],
    });

    let rendered = serde_json::to_string_pretty(&manifest)?;
    fs::write(&manifest_path, rendered)?;
    set_native_host_manifest_permissions(&manifest_path)?;

    Ok(manifest_path)
}

fn native_host_manifest_path() -> Result<PathBuf> {
    let home = current_user_home_dir().ok_or_else(|| {
        TermKeyError::ConfigError("Could not determine the current user home directory.".into())
    })?;

    Ok(native_host_manifest_path_for_home(&home))
}

fn native_host_manifest_path_for_home(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("Google")
        .join("Chrome")
        .join("NativeMessagingHosts")
        .join(format!("{CHROME_NATIVE_HOST_NAME}.json"))
}

fn current_user_home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

fn native_host_manifest_status(
    manifest_path: &Path,
    expected_binary: Option<&Path>,
) -> Result<String> {
    if !manifest_path.exists() {
        return Ok("missing".to_string());
    }

    let contents = fs::read_to_string(manifest_path)?;
    let parsed: Value = match serde_json::from_str(&contents) {
        Ok(parsed) => parsed,
        Err(_) => return Ok("invalid manifest; run `termkey browser repair`".to_string()),
    };
    let expected_origin = format!("chrome-extension://{}/", CHROME_EXTENSION_ID);
    let allowed_origins = parsed
        .get("allowed_origins")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            TermKeyError::ConfigError(
                "Chrome native host manifest is missing allowed_origins.".into(),
            )
        });
    let Ok(allowed_origins) = allowed_origins else {
        return Ok("missing extension origin; run `termkey browser repair`".to_string());
    };

    let has_expected_origin = allowed_origins
        .iter()
        .filter_map(Value::as_str)
        .any(|origin| origin == expected_origin);

    if !has_expected_origin {
        return Ok("stale extension ID; run `termkey browser repair`".to_string());
    }

    let configured_path = parsed
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if configured_path.is_empty() {
        return Ok("stale binary path; run `termkey browser repair`".to_string());
    }

    if !Path::new(configured_path).exists() {
        return Ok("missing binary; run `termkey browser repair`".to_string());
    }

    if let Some(expected_binary) = expected_binary {
        let configured = fs::canonicalize(configured_path).ok();
        let expected = fs::canonicalize(expected_binary).ok();
        if configured.is_none() || configured != expected {
            return Ok("stale native host path; run `termkey browser repair`".to_string());
        }
    }

    Ok("ready".to_string())
}

fn native_host_protocol_status(binary: &Path) -> String {
    let output = match Command::new(binary).arg("--protocol-info").output() {
        Ok(output) if output.status.success() => output,
        _ => return "unavailable; run `termkey browser repair`".to_string(),
    };
    parse_protocol_info(&output.stdout)
}

fn parse_protocol_info(bytes: &[u8]) -> String {
    let parsed: Value = match serde_json::from_slice(bytes) {
        Ok(parsed) => parsed,
        Err(_) => return "invalid; run `termkey browser repair`".to_string(),
    };
    if parsed.get("app").and_then(Value::as_str) != Some("termkey")
        || parsed.get("version").and_then(Value::as_str) != Some(env!("CARGO_PKG_VERSION"))
        || parsed.get("protocolVersion").and_then(Value::as_u64)
            != Some(u64::from(NATIVE_PROTOCOL_VERSION))
    {
        return "version/protocol mismatch; run `termkey browser repair`".to_string();
    }
    let capabilities = parsed.get("capabilities").and_then(Value::as_array);
    if !NATIVE_CAPABILITIES.iter().all(|required| {
        capabilities.is_some_and(|items| items.iter().any(|item| item.as_str() == Some(required)))
    }) {
        return "missing capabilities; run `termkey browser repair`".to_string();
    }

    format!(
        "ready (host {}, protocol {})",
        env!("CARGO_PKG_VERSION"),
        NATIVE_PROTOCOL_VERSION
    )
}

fn set_native_host_manifest_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o644))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn browser_uninstall_removes_only_the_native_host_manifest() {
        let dir = TempDir::new().unwrap();
        let manifest_path = dir.path().join("native-host.json");
        fs::write(&manifest_path, "{}").unwrap();

        let report = remove_browser_support_at(&manifest_path).unwrap();

        assert!(report.removed_native_host_manifest);
        assert!(!manifest_path.exists());
    }

    #[test]
    fn store_identity_is_used_for_the_listing_and_native_origin() {
        assert_eq!(CHROME_EXTENSION_ID, "dancadidkgcdlfdlfpbmmiokkeedpini");
        assert_eq!(
            CHROME_WEB_STORE_URL,
            format!("https://chromewebstore.google.com/detail/{CHROME_EXTENSION_ID}")
        );
    }

    #[test]
    fn native_host_manifest_uses_the_macos_chrome_location() {
        let home = Path::new("/Users/termkey-test");

        assert_eq!(
            native_host_manifest_path_for_home(home),
            home.join("Library")
                .join("Application Support")
                .join("Google")
                .join("Chrome")
                .join("NativeMessagingHosts")
                .join("com.ryanonmars.termkey.json"),
        );
    }

    #[test]
    fn manifest_status_requires_the_current_canonical_binary_path() {
        let dir = TempDir::new().unwrap();
        let binary = dir.path().join(native_host_binary_name());
        let manifest = dir.path().join("native-host.json");
        fs::write(&binary, "host").unwrap();
        let ready = serde_json::json!({
            "path": binary,
            "allowed_origins": [format!("chrome-extension://{CHROME_EXTENSION_ID}/")],
        });
        fs::write(&manifest, serde_json::to_vec(&ready).unwrap()).unwrap();

        assert_eq!(
            native_host_manifest_status(&manifest, Some(&binary)).unwrap(),
            "ready"
        );

        let other_binary = dir.path().join("other-native-host");
        fs::write(&other_binary, "other").unwrap();
        let stale = native_host_manifest_status(&manifest, Some(&other_binary)).unwrap();
        assert!(stale.contains("stale native host path"));
        assert!(stale.contains("browser repair"));
    }

    #[test]
    fn manifest_status_reports_malformed_metadata_as_repairable() {
        let dir = TempDir::new().unwrap();
        let manifest = dir.path().join("native-host.json");
        fs::write(&manifest, "{not-json").unwrap();

        let status = native_host_manifest_status(&manifest, None).unwrap();
        assert!(status.contains("invalid manifest"));
        assert!(status.contains("browser repair"));
    }

    #[test]
    fn protocol_info_parser_rejects_version_protocol_and_capability_skew() {
        let ready = serde_json::json!({
            "app": "termkey",
            "version": env!("CARGO_PKG_VERSION"),
            "protocolVersion": NATIVE_PROTOCOL_VERSION,
            "capabilities": NATIVE_CAPABILITIES,
        });
        assert!(parse_protocol_info(&serde_json::to_vec(&ready).unwrap()).starts_with("ready"));

        for stale in [
            serde_json::json!({
                "app": "termkey",
                "version": "0.0.1",
                "protocolVersion": NATIVE_PROTOCOL_VERSION,
                "capabilities": NATIVE_CAPABILITIES,
            }),
            serde_json::json!({
                "app": "termkey",
                "version": env!("CARGO_PKG_VERSION"),
                "protocolVersion": 1,
                "capabilities": NATIVE_CAPABILITIES,
            }),
            serde_json::json!({
                "app": "termkey",
                "version": env!("CARGO_PKG_VERSION"),
                "protocolVersion": NATIVE_PROTOCOL_VERSION,
                "capabilities": ["opaque-match-handles"],
            }),
        ] {
            let status = parse_protocol_info(&serde_json::to_vec(&stale).unwrap());
            assert!(status.contains("browser repair"));
        }
    }
}
