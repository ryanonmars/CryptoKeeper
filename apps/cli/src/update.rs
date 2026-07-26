use std::{
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RELEASES_PAGE_URL: &str = "https://github.com/ryanonmars/termkey/releases";
const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/ryanonmars/termkey/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub latest_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    Unknown,
    UpToDate,
    Available(UpdateInfo),
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

pub fn spawn_update_check() -> Receiver<UpdateStatus> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let status = get_update_status();
        let _ = tx.send(status);
    });

    rx
}

pub fn get_update_status() -> UpdateStatus {
    check_for_updates().unwrap_or(UpdateStatus::Unknown)
}

pub fn release_page_url_for_version(version: &str) -> String {
    format!("{}/tag/v{}", RELEASES_PAGE_URL, normalize_version(version))
}

fn check_for_updates() -> Result<UpdateStatus, reqwest::Error> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()?;

    let release = client
        .get(LATEST_RELEASE_API_URL)
        .header(USER_AGENT, format!("termkey/{}", CURRENT_VERSION))
        .header(ACCEPT, "application/vnd.github+json")
        .send()?
        .error_for_status()?
        .json::<GitHubRelease>()?;

    if is_newer_version(&release.tag_name, CURRENT_VERSION) {
        Ok(UpdateStatus::Available(UpdateInfo {
            latest_version: normalize_version(&release.tag_name).to_string(),
        }))
    } else {
        Ok(UpdateStatus::UpToDate)
    }
}

fn normalize_version(version: &str) -> &str {
    version.trim().trim_start_matches(['v', 'V'])
}

fn parse_version_parts(version: &str) -> Option<Vec<u64>> {
    let normalized = normalize_version(version).split(['-', '+']).next()?.trim();

    if normalized.is_empty() {
        return None;
    }

    normalized
        .split('.')
        .map(|segment| segment.parse::<u64>().ok())
        .collect()
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    let Some(latest_parts) = parse_version_parts(latest) else {
        return false;
    };
    let Some(current_parts) = parse_version_parts(current) else {
        return false;
    };

    let max_len = latest_parts.len().max(current_parts.len());

    for idx in 0..max_len {
        let latest_part = *latest_parts.get(idx).unwrap_or(&0);
        let current_part = *current_parts.get(idx).unwrap_or(&0);

        if latest_part > current_part {
            return true;
        }
        if latest_part < current_part {
            return false;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::is_newer_version;

    #[test]
    fn detects_newer_patch_versions() {
        assert!(is_newer_version("0.2.22", "0.2.21"));
    }

    #[test]
    fn handles_v_prefixes_and_missing_segments() {
        assert!(is_newer_version("v0.3.0", "0.2.21"));
        assert!(!is_newer_version("0.2", "0.2.0"));
    }

    #[test]
    fn ignores_older_and_invalid_versions() {
        assert!(!is_newer_version("0.2.20", "0.2.21"));
        assert!(!is_newer_version("latest", "0.2.21"));
    }
}
