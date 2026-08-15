use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::cli::BrowserCommands;
use crate::error::{Result, TermKeyError};
use crate::native::site::{NATIVE_CAPABILITIES, NATIVE_PROTOCOL_VERSION};
use crate::ui::borders::print_success;

const CHROME_EXTENSION_ID: &str = "fpnkkpgaogkddgangnphpgbbfdcpfjah";
const CHROME_NATIVE_HOST_NAME: &str = "com.ryanonmars.termkey";
const EXTENSION_SOURCE_ENV: &str = "TERMKEY_BROWSER_EXTENSION_SOURCE";
const NATIVE_HOST_BINARY_ENV: &str = "TERMKEY_NATIVE_HOST_BINARY";

pub fn run(command: &BrowserCommands) -> Result<()> {
    match command {
        BrowserCommands::Install => install_browser_support("installed"),
        BrowserCommands::Repair => install_browser_support("reinstalled"),
        BrowserCommands::Status => print_status(),
        BrowserCommands::Uninstall => {
            let report = uninstall_browser_support()?;
            if report.removed_extension || report.removed_native_host_manifest {
                print_success("Chrome integration removed.");
            } else {
                println!("  Chrome integration is already removed.");
            }
            Ok(())
        }
    }
}

#[derive(Default)]
pub(crate) struct BrowserUninstallReport {
    pub removed_extension: bool,
    pub removed_native_host_manifest: bool,
}

pub(crate) fn uninstall_browser_support() -> Result<BrowserUninstallReport> {
    remove_browser_support_at(&managed_extension_dir(), &native_host_manifest_path()?)
}

fn remove_browser_support_at(
    managed_extension_dir: &Path,
    native_host_manifest: &Path,
) -> Result<BrowserUninstallReport> {
    let mut report = BrowserUninstallReport::default();
    if managed_extension_dir.exists() {
        fs::remove_dir_all(managed_extension_dir)?;
        report.removed_extension = true;
    }
    if native_host_manifest.exists() {
        fs::remove_file(native_host_manifest)?;
        report.removed_native_host_manifest = true;
    }
    Ok(report)
}

fn install_browser_support(action: &str) -> Result<()> {
    let source_dir = locate_extension_source()?;
    let native_host_binary = locate_native_host_binary()?;
    let managed_extension_dir = managed_extension_dir();

    sync_directory(&source_dir, &managed_extension_dir)?;
    let manifest_path = install_native_host_manifest(&native_host_binary)?;

    print_success(&format!("Chrome integration {}.", action));
    println!();
    println!("  Stable Chrome extension ID: {}", CHROME_EXTENSION_ID);
    println!(
        "  Extension folder for Load unpacked: {}",
        managed_extension_dir.display()
    );
    println!("  Native host manifest: {}", manifest_path.display());
    println!();
    println!("  Next step in Chrome:");
    println!("  1. Open chrome://extensions");
    println!("  2. Turn on Developer mode");
    println!("  3. Click Load unpacked");
    println!("  4. Select {}", managed_extension_dir.display());
    println!();
    println!("  Run `termkey browser status` any time to verify the setup paths.");

    Ok(())
}

fn print_status() -> Result<()> {
    let bundled_extension_source = locate_extension_source().ok();
    let native_host_binary = locate_native_host_binary().ok();
    let managed_extension_dir = managed_extension_dir();
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
    println!("  Chrome extension ID: {}", CHROME_EXTENSION_ID);
    println!(
        "  Bundled extension source: {}",
        describe_optional_path(bundled_extension_source.as_deref())
    );
    println!(
        "  Managed extension folder: {}",
        describe_existing_path(&managed_extension_dir)
    );
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
    println!("  Chrome still requires one manual step for non-store extensions:");
    println!("  Load unpacked from {}", managed_extension_dir.display());
    println!("  after enabling Developer mode on chrome://extensions.");
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

fn managed_extension_dir() -> PathBuf {
    if let Some(home) = current_user_home_dir() {
        return home.join("Applications").join("TermKey Browser Extension");
    }

    crate::vault::storage::vault_dir()
        .join("browser")
        .join("chrome-extension")
}

fn locate_extension_source() -> Result<PathBuf> {
    let current_exe = std::env::current_exe().map_err(TermKeyError::Io)?;
    for candidate in extension_source_candidates(&current_exe) {
        if is_extension_bundle_dir(&candidate) {
            return Ok(candidate);
        }
    }

    Err(TermKeyError::ConfigError(
        "Chrome extension bundle not found. Build it with `npm run build:extension`, or use an installer that includes browser support.".into(),
    ))
}

fn extension_source_candidates(current_exe: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let executable_paths = executable_candidate_paths(current_exe);

    if let Ok(path) = std::env::var(EXTENSION_SOURCE_ENV) {
        candidates.push(PathBuf::from(path));
    }

    for executable_path in &executable_paths {
        if let Some(exe_dir) = executable_path.parent() {
            candidates.push(exe_dir.join("browser-extension").join("chrome"));
            candidates.push(exe_dir.join("browser-extension"));

            for prefix in install_prefix_candidates(exe_dir) {
                candidates.push(
                    prefix
                        .join("share")
                        .join("termkey")
                        .join("browser-extension")
                        .join("chrome"),
                );
            }

            if let Some(repo_root) = repo_root_from_exe(exe_dir) {
                candidates.push(repo_root.join("browser-extension").join("chrome"));
                candidates.push(repo_root.join("apps").join("extension"));
            }
        }

        if let Some(resources_dir) = macos_resources_dir(executable_path) {
            candidates.push(resources_dir.join("browser-extension").join("chrome"));
        }
    }

    if let Some(resources_dir) = macos_installed_app_resources_dir() {
        candidates.push(resources_dir.join("browser-extension").join("chrome"));
    }

    candidates
}

fn is_extension_bundle_dir(path: &Path) -> bool {
    path.join("manifest.json").is_file()
        && path.join("popup.html").is_file()
        && path.join("prompt.html").is_file()
        && path.join("dist").join("background.js").is_file()
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

fn sync_directory(source: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        fs::remove_dir_all(destination)?;
    }

    fs::create_dir_all(destination)?;
    copy_directory_recursive(source, destination)?;
    Ok(())
}

fn copy_directory_recursive(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target_path = destination.join(entry.file_name());

        if file_type.is_dir() {
            fs::create_dir_all(&target_path)?;
            copy_directory_recursive(&entry.path(), &target_path)?;
        } else {
            fs::copy(entry.path(), &target_path)?;
        }
    }

    Ok(())
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
    fn extension_bundle_validation_requires_built_artifacts() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("manifest.json"), "{}").unwrap();
        fs::write(dir.path().join("popup.html"), "<!doctype html>").unwrap();
        fs::write(dir.path().join("prompt.html"), "<!doctype html>").unwrap();
        fs::create_dir_all(dir.path().join("dist")).unwrap();
        fs::write(
            dir.path().join("dist").join("background.js"),
            "console.log('ok');",
        )
        .unwrap();

        assert!(is_extension_bundle_dir(dir.path()));
    }

    #[test]
    fn sync_directory_replaces_previous_contents() {
        let source = TempDir::new().unwrap();
        let destination = TempDir::new().unwrap();

        fs::write(source.path().join("manifest.json"), "{}").unwrap();
        fs::write(source.path().join("popup.html"), "<!doctype html>").unwrap();
        fs::write(source.path().join("prompt.html"), "<!doctype html>").unwrap();
        fs::create_dir_all(source.path().join("dist")).unwrap();
        fs::write(
            source.path().join("dist").join("background.js"),
            "console.log('ok');",
        )
        .unwrap();
        fs::write(destination.path().join("old.txt"), "stale").unwrap();

        let target = destination.path().join("chrome-extension");
        sync_directory(source.path(), &target).unwrap();

        assert!(target.join("manifest.json").exists());
        assert!(target.join("dist").join("background.js").exists());
        assert!(!target.join("old.txt").exists());
    }

    #[test]
    fn browser_uninstall_removes_managed_files() {
        let dir = TempDir::new().unwrap();
        let extension_dir = dir.path().join("TermKey Browser Extension");
        let manifest_path = dir.path().join("native-host.json");
        fs::create_dir_all(&extension_dir).unwrap();
        fs::write(extension_dir.join("manifest.json"), "{}").unwrap();
        fs::write(&manifest_path, "{}").unwrap();

        let report = remove_browser_support_at(&extension_dir, &manifest_path).unwrap();

        assert!(report.removed_extension);
        assert!(report.removed_native_host_manifest);
        assert!(!extension_dir.exists());
        assert!(!manifest_path.exists());
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
