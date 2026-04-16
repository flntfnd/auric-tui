use std::time::{Duration, Instant};

const GITHUB_REPO: &str = "flntfnd/auric-tui";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub struct UpdateChecker {
    latest_version: Option<String>,
    last_check: Option<Instant>,
    checking: bool,
}

impl UpdateChecker {
    pub fn new() -> Self {
        Self {
            latest_version: None,
            last_check: None,
            checking: false,
        }
    }

    /// Start a background check if enough time has passed
    pub fn maybe_check(&mut self) -> Option<std::thread::JoinHandle<Option<String>>> {
        if self.checking {
            return None;
        }
        if let Some(last) = self.last_check {
            if last.elapsed() < CHECK_INTERVAL {
                return None;
            }
        }
        self.checking = true;
        Some(std::thread::spawn(|| check_latest_version().ok()))
    }

    /// Call with the result from the background thread
    pub fn finish_check(&mut self, version: Option<String>) {
        self.latest_version = version;
        self.last_check = Some(Instant::now());
        self.checking = false;
    }

    /// Returns Some("x.y.z") if a newer version is available
    pub fn update_available(&self, current: &str) -> Option<&str> {
        self.latest_version.as_deref().and_then(|latest| {
            if version_newer(latest, current) {
                Some(latest)
            } else {
                None
            }
        })
    }
}

fn check_latest_version() -> Result<String, String> {
    let output = std::process::Command::new("curl")
        .args([
            "-sL",
            "--max-time",
            "5",
            "-H",
            "Accept: application/vnd.github.v3+json",
            &format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest"),
        ])
        .output()
        .map_err(|e| format!("curl failed: {e}"))?;

    if !output.status.success() {
        return Err("GitHub API request failed".to_string());
    }

    let body = String::from_utf8_lossy(&output.stdout);
    extract_tag_name(&body).ok_or_else(|| "could not parse tag_name".to_string())
}

fn extract_tag_name(json: &str) -> Option<String> {
    let marker = "\"tag_name\"";
    let pos = json.find(marker)?;
    let rest = &json[pos + marker.len()..];
    let rest = rest.trim_start().strip_prefix(':')?;
    let rest = rest.trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    let tag = &rest[..end];
    Some(tag.strip_prefix('v').unwrap_or(tag).to_string())
}

fn version_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.split('.')
            .filter_map(|p| p.parse::<u32>().ok())
            .collect()
    };
    let l = parse(latest);
    let c = parse(current);
    l > c
}

/// Download and install the latest release
pub fn self_update(current_version: &str) -> Result<String, String> {
    let latest = check_latest_version()?;
    if !version_newer(&latest, current_version) {
        return Ok(format!("Already up to date (v{current_version})"));
    }

    let target = if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        "x86_64-apple-darwin"
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        "x86_64-pc-windows-msvc"
    } else {
        return Err("unsupported platform for self-update; build from source instead".to_string());
    };

    let ext = if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar.gz"
    };
    let url = format!(
        "https://github.com/{GITHUB_REPO}/releases/download/v{latest}/auric-{target}.{ext}"
    );

    let tmp_dir = std::env::temp_dir().join("auric-update");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let archive_path = tmp_dir.join(format!("auric-{target}.{ext}"));

    let status = std::process::Command::new("curl")
        .args([
            "-sL",
            "--max-time",
            "60",
            "-o",
            &archive_path.display().to_string(),
            &url,
        ])
        .status()
        .map_err(|e| format!("download failed: {e}"))?;

    if !status.success() {
        return Err(format!("failed to download {url}"));
    }

    if cfg!(target_os = "windows") {
        let _ = std::process::Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    "Expand-Archive -Force '{}' '{}'",
                    archive_path.display(),
                    tmp_dir.display()
                ),
            ])
            .status();
    } else {
        let status = std::process::Command::new("tar")
            .args([
                "xzf",
                &archive_path.display().to_string(),
                "-C",
                &tmp_dir.display().to_string(),
            ])
            .status()
            .map_err(|e| format!("extract failed: {e}"))?;
        if !status.success() {
            return Err("failed to extract archive".to_string());
        }
    }

    let new_binary = tmp_dir.join(if cfg!(target_os = "windows") {
        "auric.exe"
    } else {
        "auric"
    });
    if !new_binary.exists() {
        return Err("extracted binary not found".to_string());
    }

    let current_exe =
        std::env::current_exe().map_err(|e| format!("cannot find current executable: {e}"))?;

    let backup = current_exe.with_extension("old");
    let _ = std::fs::remove_file(&backup);
    std::fs::rename(&current_exe, &backup)
        .map_err(|e| format!("failed to backup current binary: {e}"))?;

    if let Err(e) = std::fs::copy(&new_binary, &current_exe) {
        let _ = std::fs::rename(&backup, &current_exe);
        return Err(format!("failed to install new binary: {e}"));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&current_exe, std::fs::Permissions::from_mode(0o755));
    }

    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_dir_all(&tmp_dir);

    Ok(format!(
        "Updated from v{current_version} to v{latest}. Restart auric to use the new version."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert!(version_newer("0.2.0", "0.1.0"));
        assert!(version_newer("1.0.0", "0.9.9"));
        assert!(version_newer("0.1.1", "0.1.0"));
        assert!(!version_newer("0.1.0", "0.1.0"));
        assert!(!version_newer("0.1.0", "0.2.0"));
    }

    #[test]
    fn extract_tag_from_json() {
        let json = r#"{"tag_name": "v0.2.0", "name": "Release 0.2.0"}"#;
        assert_eq!(extract_tag_name(json), Some("0.2.0".to_string()));
    }

    #[test]
    fn extract_tag_without_v_prefix() {
        let json = r#"{"tag_name": "1.0.0"}"#;
        assert_eq!(extract_tag_name(json), Some("1.0.0".to_string()));
    }

    #[test]
    fn extract_tag_missing() {
        assert_eq!(extract_tag_name(r#"{"name": "test"}"#), None);
    }

    #[test]
    fn update_checker_available() {
        let mut checker = UpdateChecker::new();
        checker.finish_check(Some("0.2.0".to_string()));
        assert_eq!(checker.update_available("0.1.0"), Some("0.2.0"));
        assert_eq!(checker.update_available("0.2.0"), None);
        assert_eq!(checker.update_available("0.3.0"), None);
    }

    #[test]
    fn update_checker_none() {
        let mut checker = UpdateChecker::new();
        checker.finish_check(None);
        assert_eq!(checker.update_available("0.1.0"), None);
    }
}
