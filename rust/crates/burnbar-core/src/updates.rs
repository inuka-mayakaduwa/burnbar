//! Public stable release metadata only; BurnBar never installs updates itself
//! (Go: `internal/updates/`).

use serde::Deserialize;
use std::io::Read;
use std::time::Duration;

pub const ENDPOINT: &str = "https://api.github.com/repos/nooblk-98/burnbar/releases/latest";
pub const RELEASES: &str = "https://github.com/nooblk-98/burnbar/releases";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

/// Parse a stable `MAJOR.MINOR.PATCH` version, rejecting prerelease and build
/// metadata, leading zeros and non-numeric segments.
pub fn stable(value: &str) -> Option<[i64; 3]> {
    let value = value.strip_prefix('v').unwrap_or(value);
    if value.contains(['-', '+']) {
        return None;
    }
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut result = [0_i64; 3];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
            return None;
        }
        if !part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        result[index] = part.parse().ok()?;
    }
    Some(result)
}

pub fn newer(current: &str, candidate: &str) -> bool {
    let (Some(a), Some(b)) = (stable(current), stable(candidate)) else {
        return false;
    };
    for index in 0..3 {
        if b[index] != a[index] {
            return b[index] > a[index];
        }
    }
    false
}

fn valid_release_url(url: &str, tag: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.host_str() == Some("github.com")
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parsed.path() == format!("/nooblk-98/burnbar/releases/tag/{tag}")
}

#[derive(Deserialize)]
struct Payload {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

fn decode(body: &[u8]) -> Result<Release, String> {
    let payload: Payload =
        serde_json::from_slice(body).map_err(|_| "invalid release metadata".to_string())?;
    if stable(&payload.tag_name).is_none() || payload.draft || payload.prerelease {
        return Err("no stable release available".into());
    }
    if !valid_release_url(&payload.html_url, &payload.tag_name) {
        return Err("invalid release link".into());
    }
    Ok(Release {
        version: payload
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&payload.tag_name)
            .to_string(),
        url: payload.html_url,
    })
}

/// Fetch the latest stable release. `endpoint` is overridable for tests.
pub fn check_from(endpoint: &str) -> Result<Release, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "could not reach GitHub".to_string())?;
    let response = client
        .get(endpoint)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "BurnBar update check")
        .send()
        .map_err(|_| "could not reach GitHub".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub returned HTTP {}; try again later",
            response.status().as_u16()
        ));
    }
    let mut body = Vec::new();
    response
        .take(256 * 1024 + 1)
        .read_to_end(&mut body)
        .map_err(|_| "could not read release metadata".to_string())?;
    if body.len() > 256 * 1024 {
        return Err("could not read release metadata".into());
    }
    decode(&body)
}

pub fn check() -> Result<Release, String> {
    check_from(ENDPOINT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_stable_versions() {
        assert_eq!(stable("v0.4.1"), Some([0, 4, 1]));
        assert_eq!(stable("1.2.3"), Some([1, 2, 3]));
        assert_eq!(stable("v0.4.1-beta"), None);
        assert_eq!(stable("v0.4.1+build"), None);
        assert_eq!(stable("v01.2.3"), None);
        assert_eq!(stable("1.2"), None);
        assert!(newer("0.4.1", "0.4.2"));
        assert!(newer("0.4.1", "0.5.0"));
        assert!(!newer("0.4.2", "0.4.2"));
        assert!(!newer("0.5.0", "0.4.9"));
        assert!(!newer("dev", "1.0.0"));
    }

    #[test]
    fn decodes_only_valid_stable_releases() {
        let body = br#"{"tag_name":"v0.5.0","html_url":"https://github.com/nooblk-98/burnbar/releases/tag/v0.5.0"}"#;
        let release = decode(body).unwrap();
        assert_eq!(release.version, "0.5.0");
        assert!(release.url.ends_with("v0.5.0"));

        let prerelease = br#"{"tag_name":"v0.5.0","html_url":"https://github.com/nooblk-98/burnbar/releases/tag/v0.5.0","prerelease":true}"#;
        assert_eq!(
            decode(prerelease).unwrap_err(),
            "no stable release available"
        );

        let wrong_host = br#"{"tag_name":"v0.5.0","html_url":"https://evil.example/nooblk-98/burnbar/releases/tag/v0.5.0"}"#;
        assert_eq!(decode(wrong_host).unwrap_err(), "invalid release link");
    }
}
