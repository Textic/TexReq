use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RELEASES_PAGE_URL: &str = "https://github.com/Textic/TexReq/releases";
const LATEST_RELEASE_API_URL: &str = "https://api.github.com/repos/Textic/TexReq/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub tag_name: String,
    pub release_url: String,
    pub download_url: String,
    pub published_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(UpdateInfo),
    Failed(String),
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

pub fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let core = trimmed.split(['-', '+']).next()?.trim();
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer_version(current: &str, latest: &str) -> bool {
    match (parse_semver(current), parse_semver(latest)) {
        (Some(cur), Some(lat)) => lat > cur,
        _ => false,
    }
}

pub fn pick_asset_url_for_target<'a>(
    assets: &'a [(String, String)],
    os: &str,
    arch: &str,
) -> Option<&'a str> {
    let find = |pred: &dyn Fn(&str) -> bool| -> Option<&'a str> {
        assets
            .iter()
            .find(|(name, _)| pred(&name.to_ascii_lowercase()))
            .map(|(_, url)| url.as_str())
    };

    match os {
        "windows" => find(&|n| n.contains("windows") && n.ends_with("-setup.exe"))
            .or_else(|| find(&|n| n.contains("windows") && n.ends_with(".exe")))
            .or_else(|| find(&|n| n.contains("windows") && n.ends_with(".zip"))),
        "macos" => {
            if arch == "aarch64" {
                find(&|n| n.contains("macos") && (n.contains("arm64") || n.contains("aarch64")) && n.ends_with(".dmg"))
                    .or_else(|| find(&|n| n.contains("macos") && (n.contains("arm64") || n.contains("aarch64")) && n.ends_with(".zip")))
                    .or_else(|| find(&|n| n.contains("macos") && n.ends_with(".dmg")))
            } else {
                find(&|n| n.contains("macos") && (n.contains("x86_64") || n.contains("intel")) && n.ends_with(".dmg"))
                    .or_else(|| find(&|n| n.contains("macos") && (n.contains("x86_64") || n.contains("intel")) && n.ends_with(".zip")))
                    .or_else(|| find(&|n| n.contains("macos") && n.ends_with(".dmg")))
            }
        }
        "linux" => find(&|n| n.contains("linux") && n.ends_with(".deb"))
            .or_else(|| find(&|n| n.contains("linux") && n.ends_with(".tar.gz")))
            .or_else(|| find(&|n| n.contains("linux"))),
        _ => None,
    }
}

pub async fn check_for_updates() -> Result<Option<UpdateInfo>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(format!("TexReq/{CURRENT_VERSION}"))
        .build()
        .map_err(|e| format!("Failed to initialize HTTP client: {e}"))?;

    let resp = client
        .get(LATEST_RELEASE_API_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    if resp.status().as_u16() == 404 {
        return Ok(None);
    }

    if !resp.status().is_success() {
        return Err(format!("GitHub API returned HTTP {}", resp.status().as_u16()));
    }

    let rel: GithubRelease = resp
        .json()
        .await
        .map_err(|e| format!("Invalid GitHub release payload: {e}"))?;

    if rel.draft || rel.prerelease || !is_newer_version(CURRENT_VERSION, &rel.tag_name) {
        return Ok(None);
    }

    let version = rel.tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    let pairs: Vec<(String, String)> = rel
        .assets
        .iter()
        .map(|a| (a.name.clone(), a.browser_download_url.clone()))
        .collect();
    let download_url = pick_asset_url_for_target(&pairs, std::env::consts::OS, std::env::consts::ARCH)
        .unwrap_or(&rel.html_url)
        .to_string();

    let published_at = rel
        .published_at
        .as_deref()
        .and_then(|s| s.split('T').next())
        .unwrap_or("")
        .to_string();

    Ok(Some(UpdateInfo {
        version,
        tag_name: rel.tag_name,
        release_url: rel.html_url,
        download_url,
        published_at,
    }))
}

pub fn open_url_in_browser(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("Refusing to open non-HTTP URL".to_string());
    }

    #[cfg(target_os = "windows")]
    let status = Command::new("explorer.exe").arg(trimmed).spawn();
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(trimmed).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = Command::new("xdg-open").arg(trimmed).spawn();

    status
        .map(|_| ())
        .map_err(|e| format!("Failed to open browser: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_tags_correctly() {
        assert_eq!(parse_semver("v0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("1.2.3-rc.1"), Some((1, 2, 3)));
        assert!(is_newer_version("0.1.0", "v0.1.1"));
        assert!(is_newer_version("0.1.9", "v0.2.0"));
        assert!(!is_newer_version("0.1.0", "v0.1.0"));
        assert!(!is_newer_version("0.2.0", "v0.1.9"));
    }

    #[test]
    fn picks_matching_asset_per_platform() {
        let assets = vec![
            (
                "texreq-v0.1.1-windows-x86_64.exe".to_string(),
                "https://example.com/win.exe".to_string(),
            ),
            (
                "texreq-v0.1.1-windows-x86_64-setup.exe".to_string(),
                "https://example.com/win-setup.exe".to_string(),
            ),
            (
                "texreq-v0.1.1-linux-x86_64.tar.gz".to_string(),
                "https://example.com/linux.tar.gz".to_string(),
            ),
            (
                "texreq-v0.1.1-linux-x86_64.deb".to_string(),
                "https://example.com/linux.deb".to_string(),
            ),
            (
                "texreq-v0.1.1-macos-arm64.zip".to_string(),
                "https://example.com/mac-arm.zip".to_string(),
            ),
            (
                "texreq-v0.1.1-macos-arm64.dmg".to_string(),
                "https://example.com/mac-arm.dmg".to_string(),
            ),
            (
                "texreq-v0.1.1-macos-x86_64.dmg".to_string(),
                "https://example.com/mac-x64.dmg".to_string(),
            ),
        ];

        assert_eq!(
            pick_asset_url_for_target(&assets, "windows", "x86_64"),
            Some("https://example.com/win-setup.exe")
        );
        assert_eq!(
            pick_asset_url_for_target(&assets, "linux", "x86_64"),
            Some("https://example.com/linux.deb")
        );
        assert_eq!(
            pick_asset_url_for_target(&assets, "macos", "aarch64"),
            Some("https://example.com/mac-arm.dmg")
        );
        assert_eq!(
            pick_asset_url_for_target(&assets, "macos", "x86_64"),
            Some("https://example.com/mac-x64.dmg")
        );
    }
}
