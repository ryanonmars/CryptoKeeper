use serde::Serialize;
use sha2::{Digest, Sha256};
use url::{Host, Url};

use crate::error::{Result, TermKeyError};
use crate::vault::model::{Entry, SecretType, VaultData};

pub const NATIVE_PROTOCOL_VERSION: u32 = 2;
pub const NATIVE_CAPABILITIES: &[&str] = &[
    "opaque-match-handles",
    "document-token-binding",
    "origin-only-save",
    "bounded-native-output",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpsOrigin(String);

impl HttpsOrigin {
    pub fn parse(input: &str) -> Result<Self> {
        let url = parse_web_url(input, false)?;
        Ok(Self(url.origin().ascii_serialization()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn host(&self) -> Host<String> {
        Url::parse(&self.0)
            .expect("HttpsOrigin always contains a parsed URL")
            .host()
            .expect("HttpsOrigin always contains a host")
            .to_owned()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteMatch {
    pub id: String,
    pub name: String,
    pub username: Option<String>,
    pub match_type: &'static str,
    pub has_secondary_password: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SiteRule {
    Origin(HttpsOrigin),
    Host(Host<String>),
    Domain(String),
}

impl SiteRule {
    fn match_type(&self, origin: &HttpsOrigin) -> Option<&'static str> {
        match self {
            Self::Origin(rule_origin) if rule_origin == origin => Some("exact_origin"),
            Self::Host(rule_host) if *rule_host == origin.host() => Some("exact_host"),
            Self::Domain(rule_domain)
                if registrable_domain(&origin.host()).as_deref() == Some(rule_domain.as_str()) =>
            {
                Some("registrable_domain")
            }
            _ => None,
        }
    }
}

pub fn entry_authorizes_origin(entry: &Entry, origin: &HttpsOrigin) -> bool {
    effective_rules(entry)
        .iter()
        .any(|rule| rule.match_type(origin).is_some())
}

pub fn find_site_matches(vault: &VaultData, origin: &HttpsOrigin) -> Vec<SiteMatch> {
    let mut matches: Vec<_> = vault
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.secret_type == SecretType::Password)
        .filter_map(|(index, entry)| {
            let match_type = effective_rules(entry)
                .iter()
                .filter_map(|rule| rule.match_type(origin))
                .max_by_key(|match_type| match_rank(match_type))?;

            Some((
                match_rank(match_type),
                index,
                SiteMatch {
                    id: entry_fingerprint(entry),
                    name: entry.name.clone(),
                    username: entry.username.clone(),
                    match_type,
                    has_secondary_password: entry.has_secondary_password,
                },
            ))
        })
        .collect();

    matches.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.2.name.to_lowercase().cmp(&right.2.name.to_lowercase()))
            .then_with(|| left.1.cmp(&right.1))
    });
    matches
        .into_iter()
        .map(|(_, _, site_match)| site_match)
        .collect()
}

pub fn entry_fingerprint(entry: &Entry) -> String {
    let encoded = serde_json::to_vec(entry)
        .expect("validated vault entries always serialize for identity checks");
    hex::encode(Sha256::digest(encoded))
}

fn effective_rules(entry: &Entry) -> Vec<SiteRule> {
    let parsed: Vec<_> = if entry.site_rules.is_empty() {
        entry
            .url
            .as_deref()
            .and_then(default_origin_from_entry_url)
            .map(SiteRule::Origin)
            .into_iter()
            .collect()
    } else {
        entry
            .site_rules
            .iter()
            .filter_map(|rule| parse_explicit_rule(rule))
            .collect()
    };

    let mut deduped = Vec::new();
    for rule in parsed {
        if !deduped.contains(&rule) {
            deduped.push(rule);
        }
    }
    deduped
}

fn default_origin_from_entry_url(input: &str) -> Option<HttpsOrigin> {
    parse_web_url(input, true)
        .ok()
        .map(|url| HttpsOrigin(url.origin().ascii_serialization()))
}

fn parse_explicit_rule(input: &str) -> Option<SiteRule> {
    if input.trim() != input {
        return None;
    }

    if let Some(value) = input.strip_prefix("origin:") {
        return HttpsOrigin::parse(value).ok().map(SiteRule::Origin);
    }
    if let Some(value) = input.strip_prefix("host:") {
        return parse_host(value).map(SiteRule::Host);
    }
    if let Some(value) = input.strip_prefix("domain:") {
        let host = parse_host(value)?;
        return registrable_domain(&host).map(SiteRule::Domain);
    }
    None
}

fn parse_host(input: &str) -> Option<Host<String>> {
    if input.is_empty()
        || input.trim() != input
        || input.contains('%')
        || contains_ambiguous_raw_char(input)
    {
        return None;
    }
    normalize_host(Host::parse(input).ok()?)
}

fn registrable_domain(host: &Host<String>) -> Option<String> {
    let Host::Domain(domain) = host else {
        return None;
    };
    psl2::registrable_domain(domain)
}

fn parse_web_url(input: &str, allow_page_components: bool) -> Result<Url> {
    if input.is_empty() || input.trim() != input || contains_ambiguous_raw_char(input) {
        return Err(invalid_origin());
    }

    let (_, authority_and_rest) = input.split_once("://").ok_or_else(invalid_origin)?;
    let authority_end = authority_and_rest
        .find(['/', '?', '#'])
        .unwrap_or(authority_and_rest.len());
    let authority = &authority_and_rest[..authority_end];
    let page_components = &authority_and_rest[authority_end..];
    if authority.contains('@')
        || authority.contains('%')
        || (!allow_page_components && !matches!(page_components, "" | "/"))
    {
        return Err(invalid_origin());
    }

    let mut url = Url::parse(input).map_err(|_| invalid_origin())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.cannot_be_a_base()
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || (!allow_page_components
            && (url.path() != "/" || url.query().is_some() || url.fragment().is_some()))
    {
        return Err(invalid_origin());
    }

    let host = normalize_host(url.host().expect("host checked above").to_owned())
        .ok_or_else(invalid_origin)?;
    url.set_host(Some(&host.to_string()))
        .map_err(|_| invalid_origin())?;
    Ok(url)
}

fn contains_ambiguous_raw_char(input: &str) -> bool {
    input.contains('\\')
        || input
            .chars()
            .any(|character| character.is_ascii_control() || character.is_ascii_whitespace())
}

fn normalize_host(host: Host<String>) -> Option<Host<String>> {
    match host {
        Host::Domain(domain) => {
            let without_dot = domain.strip_suffix('.').unwrap_or(&domain);
            if without_dot.is_empty() || without_dot.ends_with('.') {
                return None;
            }
            Host::parse(without_dot).ok()
        }
        address => Some(address),
    }
}

fn match_rank(match_type: &str) -> u8 {
    match match_type {
        "exact_origin" => 3,
        "exact_host" => 2,
        "registrable_domain" => 1,
        _ => 0,
    }
}

fn invalid_origin() -> TermKeyError {
    TermKeyError::InvalidEntry("site origin must be a canonical HTTP or HTTPS origin".to_string())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use crate::vault::model::{Entry, SecretType, VaultData};

    use super::{entry_authorizes_origin, find_site_matches, HttpsOrigin};

    fn password_entry(name: &str, url: Option<&str>, site_rules: &[&str]) -> Entry {
        Entry {
            name: name.to_string(),
            secret: "secret".to_string(),
            secret_type: SecretType::Password,
            network: "Password".to_string(),
            public_address: None,
            username: Some(format!("{name}@example.com")),
            url: url.map(str::to_string),
            site_rules: site_rules.iter().map(|rule| (*rule).to_string()).collect(),
            notes: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            has_secondary_password: false,
            entry_key_wrapped: None,
            entry_key_nonce: None,
            entry_key_salt: None,
            encrypted_secret: None,
            encrypted_secret_nonce: None,
        }
    }

    #[test]
    fn rejects_urls_with_userinfo() {
        assert!(HttpsOrigin::parse("https://user@example.com").is_err());
        assert!(HttpsOrigin::parse("https://user:password@example.com").is_err());
        assert!(HttpsOrigin::parse("https://@example.com").is_err());

        let origin = HttpsOrigin::parse("https://example.com").unwrap();
        assert!(!entry_authorizes_origin(
            &password_entry("Userinfo", Some("https://user@example.com"), &[]),
            &origin
        ));
    }

    #[test]
    fn http_origins_are_canonical_and_do_not_match_https() {
        assert_eq!(
            HttpsOrigin::parse("http://EXAMPLE.test:80/")
                .unwrap()
                .as_str(),
            "http://example.test"
        );

        let entry = password_entry("HTTP", Some("http://example.test/login"), &[]);
        assert!(entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("http://example.test").unwrap()
        ));
        assert!(!entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://example.test").unwrap()
        ));
    }

    #[test]
    fn http_ip_origin_preserves_non_default_port() {
        let entry = password_entry("qBittorrent", Some("http://192.168.4.64:8080"), &[]);

        assert!(entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("http://192.168.4.64:8080").unwrap()
        ));
        assert!(!entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("http://192.168.4.64").unwrap()
        ));
    }

    #[test]
    fn exact_origin_requires_scheme_host_and_port_match() {
        let entry = password_entry("Admin", Some("https://admin.example.com:8443/login"), &[]);

        assert!(entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://admin.example.com:8443").unwrap()
        ));
        assert!(!entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://admin.example.com").unwrap()
        ));
        assert!(!entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://www.example.com:8443").unwrap()
        ));
    }

    #[test]
    fn github_io_siblings_never_share_registrable_domain() {
        let entry = password_entry("Alice Pages", None, &["domain:alice.github.io"]);

        assert!(entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://docs.alice.github.io").unwrap()
        ));
        assert!(!entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://bob.github.io").unwrap()
        ));
    }

    #[test]
    fn explicit_domain_rule_uses_private_psl_entries() {
        let entry = password_entry("Project Pages", None, &["domain:login.project.github.io"]);

        assert!(entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://assets.project.github.io").unwrap()
        ));
        assert!(!entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://project.example.com").unwrap()
        ));
    }

    #[test]
    fn default_rule_does_not_match_subdomains() {
        let entry = password_entry("Account", Some("https://example.com/login?next=home"), &[]);

        assert!(entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://example.com").unwrap()
        ));
        assert!(!entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://app.example.com").unwrap()
        ));
    }

    #[test]
    fn origin_input_is_canonicalized_but_must_not_contain_page_components() {
        assert_eq!(
            HttpsOrigin::parse("https://EXAMPLE.com:443/")
                .unwrap()
                .as_str(),
            "https://example.com"
        );
        assert_eq!(
            HttpsOrigin::parse("https://bücher.example")
                .unwrap()
                .as_str(),
            "https://xn--bcher-kva.example"
        );
        assert_eq!(
            HttpsOrigin::parse("https://example.com.").unwrap().as_str(),
            "https://example.com"
        );
        assert!(HttpsOrigin::parse(" https://example.com").is_err());
        assert!(HttpsOrigin::parse("https://example.com/login").is_err());
        assert!(HttpsOrigin::parse("https://example.com/.").is_err());
        assert!(HttpsOrigin::parse("https://example.com/..").is_err());
        assert!(HttpsOrigin::parse("https://example.com?next=home").is_err());
        assert!(HttpsOrigin::parse("https://example.com#login").is_err());
    }

    #[test]
    fn origin_input_rejects_ambiguous_raw_authority_syntax() {
        for input in [
            r"https://example.com\",
            r"https://example.com\.",
            r"https://example.com\..",
            "https://exa\tmple.com",
            "https://exa\nmple.com",
            "https://exa\rmple.com",
            "https://example%2ecom",
            "https://example%2Ecom",
            "https://example.com%2e",
            "https://example.com%2E",
        ] {
            assert!(
                HttpsOrigin::parse(input).is_err(),
                "ambiguous origin input was accepted: {input:?}"
            );
        }
    }

    #[test]
    fn default_entry_urls_with_ambiguous_authorities_authorize_nothing() {
        let origin = HttpsOrigin::parse("https://example.com").unwrap();

        for entry_url in [
            r"https://example.com\",
            r"https://example.com\.",
            r"https://example.com\..",
            "https://exa\tmple.com",
            "https://exa\nmple.com",
            "https://exa\rmple.com",
            "https://example%2ecom",
            "https://example%2Ecom",
            "https://example.com%2e",
            "https://example.com%2E",
        ] {
            assert!(
                !entry_authorizes_origin(
                    &password_entry("Ambiguous", Some(entry_url), &[]),
                    &origin
                ),
                "ambiguous entry URL authorized the origin: {entry_url:?}"
            );
        }
    }

    #[test]
    fn explicit_host_and_domain_rules_reject_ambiguous_raw_values() {
        let origin = HttpsOrigin::parse("https://example.com").unwrap();

        for value in [
            "example%2ecom",
            "example%2Ecom",
            r"example.com\",
            "exa\tmple.com",
            "exa\nmple.com",
            "exa\rmple.com",
        ] {
            for prefix in ["host:", "domain:"] {
                let rule = format!("{prefix}{value}");
                assert!(
                    !entry_authorizes_origin(
                        &password_entry("Ambiguous Rule", None, &[&rule]),
                        &origin
                    ),
                    "ambiguous explicit rule authorized the origin: {rule:?}"
                );
            }
        }
    }

    #[test]
    fn default_entry_url_allows_percent_encoding_in_page_components() {
        let entry = password_entry(
            "Encoded Path",
            Some("https://example.com/login%2Fredirect?next=%2Fhome#section%201"),
            &[],
        );

        assert!(entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://example.com").unwrap()
        ));
    }

    #[test]
    fn default_https_port_is_the_same_origin_and_non_default_ports_are_distinct() {
        let default = HttpsOrigin::parse("https://example.com").unwrap();
        let explicit_default = HttpsOrigin::parse("https://example.com:443").unwrap();
        let non_default = HttpsOrigin::parse("https://example.com:444").unwrap();

        assert_eq!(default, explicit_default);
        assert_ne!(default, non_default);
        assert_eq!(non_default.as_str(), "https://example.com:444");
    }

    #[test]
    fn explicit_host_rule_matches_only_the_exact_host_and_ignores_port() {
        let entry = password_entry("Host Rule", None, &["host:login.example.com"]);

        assert!(entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://login.example.com").unwrap()
        ));
        assert!(entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://login.example.com:8443").unwrap()
        ));
        assert!(!entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://sub.login.example.com").unwrap()
        ));
    }

    #[test]
    fn explicit_origin_rule_is_exact_and_canonical() {
        let entry = password_entry("Origin Rule", None, &["origin:https://BÜCHER.example:443/"]);

        assert!(entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://xn--bcher-kva.example").unwrap()
        ));
        assert!(!entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://xn--bcher-kva.example:444").unwrap()
        ));
    }

    #[test]
    fn malformed_explicit_rules_and_public_suffixes_do_not_authorize() {
        let origin = HttpsOrigin::parse("https://login.example.com").unwrap();
        for rule in [
            "https://login.example.com",
            "origin:http://login.example.com",
            "origin:https://login.example.com/path",
            "host:https://login.example.com",
            "host:login.example.com:443",
            "host:user@login.example.com",
            "domain:com",
            "domain:github.io",
            "domain:https://login.example.com",
            "unknown:login.example.com",
        ] {
            assert!(
                !entry_authorizes_origin(&password_entry("Malformed", None, &[rule]), &origin),
                "malformed rule authorized the origin: {rule}"
            );
        }
    }

    #[test]
    fn ipv6_origins_are_canonical_and_host_rules_are_exact() {
        assert_eq!(
            HttpsOrigin::parse("https://[2001:0db8::1]:443")
                .unwrap()
                .as_str(),
            "https://[2001:db8::1]"
        );

        let entry = password_entry("IPv6", None, &["host:[2001:db8::1]"]);
        assert!(entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://[2001:db8::1]:8443").unwrap()
        ));
        assert!(!entry_authorizes_origin(
            &entry,
            &HttpsOrigin::parse("https://[2001:db8::2]:8443").unwrap()
        ));
    }

    #[test]
    fn matches_are_deduplicated_and_ordered_stably() {
        let vault = VaultData {
            entries: vec![
                password_entry("domain", None, &["domain:example.com"]),
                password_entry(
                    "Zulu",
                    None,
                    &["origin:https://login.example.com", "host:login.example.com"],
                ),
                password_entry("alpha", None, &["host:login.example.com"]),
                password_entry(
                    "Duplicate",
                    None,
                    &["host:login.example.com", "host:LOGIN.example.com"],
                ),
            ],
            version: 3,
            revision: 0,
        };
        let origin = HttpsOrigin::parse("https://login.example.com").unwrap();

        let matches = find_site_matches(&vault, &origin);
        let names_and_types: Vec<_> = matches
            .iter()
            .map(|site_match| (site_match.name.as_str(), site_match.match_type))
            .collect();

        assert_eq!(
            names_and_types,
            vec![
                ("Zulu", "exact_origin"),
                ("alpha", "exact_host"),
                ("Duplicate", "exact_host"),
                ("domain", "registrable_domain"),
            ]
        );
        assert_eq!(matches[0].id.len(), 64);
        assert!(matches[0].id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
