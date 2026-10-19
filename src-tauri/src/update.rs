//! The update check: one GET on the repository's "latest release" page.
//! GitHub answers with a redirect to `…/releases/tag/v1.2.0`, and only that `Location`
//! header is read – no API (and its rate limit), no JSON, no body. Nothing is sent but the
//! request itself: the User-Agent is just `VoxMD`, without version or system, and no path,
//! file or setting ever goes anywhere. Nothing is downloaded or installed; a newer version is
//! only named, with a link to its page. *When* to ask (opt-in, at most once a day) is the
//! frontend's business; this module only knows how to ask and how to read the answer.

use std::time::Duration;

const REPOSITORY: &str = "https://github.com/fly2nbc-oss/VoxMD";
/// The whole request, connecting included.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The version a redirect points at: `https://github.com/o/r/releases/tag/v1.2.0`.
/// Anything else – a pre-release suffix, a fourth number – is none.
pub fn version_in(location: &str) -> Option<String> {
    let (_, tag) = location.rsplit_once("/releases/tag/")?;
    let tag = tag.split(['?', '#']).next()?.trim_start_matches('v');
    let parts: Vec<&str> = tag.split('.').collect();
    let numeric = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    numeric.then(|| tag.to_owned())
}

/// Asks GitHub which release is the latest. HTTPS only, no redirect followed.
pub async fn latest() -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("VoxMD")
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(format!("{REPOSITORY}/releases/latest"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| format!("no redirect ({})", response.status()))?;
    version_in(location).ok_or_else(|| format!("not a release: {location}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_version_from_the_release_redirect() {
        assert_eq!(
            version_in("https://github.com/fly2nbc-oss/VoxMD/releases/tag/v1.2.0").as_deref(),
            Some("1.2.0")
        );
        assert_eq!(
            version_in("https://github.com/o/r/releases/tag/v10.0.3?x=1").as_deref(),
            Some("10.0.3")
        );
    }

    #[test]
    fn anything_that_is_not_a_plain_release_is_none() {
        for odd in [
            "https://github.com/o/r/releases/tag/v1.2",
            "https://github.com/o/r/releases/tag/v1.2.0.1",
            "https://github.com/o/r/releases/tag/v1.2.0-rc1",
            "https://github.com/o/r/releases/tag/models-1",
            "https://github.com/o/r/releases",
            "https://github.com/o/r/releases/tag/v1..0",
        ] {
            assert_eq!(version_in(odd), None, "{odd}");
        }
    }
}
