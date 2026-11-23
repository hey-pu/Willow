use semver::Version;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::Read;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

const API_RELEASES: &str = "https://api.github.com/repos/hey-pu/Willow/releases";
const WEB_RELEASES: &str = "https://github.com/hey-pu/Willow/releases";
const REQUEST_BUDGET: Duration = Duration::from_secs(10);
const PAGE_SIZE: usize = 30;
const RESPONSE_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    state: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppRelease {
    tag: String,
    version: String,
    notes: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateCheck {
    current_version: String,
    latest_release: Option<AppRelease>,
    update_available: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ReleaseError {
    RateLimited,
    Unavailable,
    UnsupportedPlatform,
}

impl GitHubRelease {
    fn windows_version(&self) -> Option<Version> {
        let version = parse_tag(&self.tag_name)?;
        if self.draft || self.prerelease || !version.pre.is_empty() {
            return None;
        }

        let installer = format!("Willow_{version}_x64-setup.exe");
        self.assets
            .iter()
            .any(|asset| {
                asset.state == "uploaded"
                    && asset.size > 0
                    && asset.name.eq_ignore_ascii_case(&installer)
            })
            .then_some(version)
    }

    fn into_app_release(self, version: &Version) -> AppRelease {
        AppRelease {
            tag: self.tag_name,
            version: version.to_string(),
            notes: self
                .body
                .unwrap_or_default()
                .trim()
                .chars()
                .take(6000)
                .collect(),
        }
    }
}

fn parse_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

fn release_url(tag: Option<&str>) -> Result<String, &'static str> {
    match tag {
        Some(tag) if parse_tag(tag).is_some() => Ok(format!("{WEB_RELEASES}/tag/{tag}")),
        Some(_) => Err("Invalid Willow release tag"),
        None => Ok(WEB_RELEASES.to_owned()),
    }
}

fn latest_windows_release(
    releases: impl IntoIterator<Item = GitHubRelease>,
) -> Option<(Version, GitHubRelease)> {
    releases
        .into_iter()
        .filter_map(|release| release.windows_version().map(|version| (version, release)))
        .max_by(|(left, _), (right, _)| left.cmp_precedence(right))
}

fn fetch<T: DeserializeOwned>(
    agent: &ureq::Agent,
    url: &str,
    deadline: Instant,
) -> Result<Option<T>, ReleaseError> {
    let timeout = deadline.saturating_duration_since(Instant::now());
    if timeout.is_zero() {
        return Err(ReleaseError::Unavailable);
    }

    let response = match agent
        .get(url)
        .set("User-Agent", "Willow")
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .timeout(timeout)
        .call()
    {
        Ok(response) if response.status() == 200 => response,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(ureq::Error::Status(429, _)) => return Err(ReleaseError::RateLimited),
        Err(ureq::Error::Status(403, response))
            if response.header("X-RateLimit-Remaining") == Some("0")
                || response.header("Retry-After").is_some() =>
        {
            return Err(ReleaseError::RateLimited);
        }
        _ => return Err(ReleaseError::Unavailable),
    };

    serde_json::from_reader(response.into_reader().take(RESPONSE_LIMIT))
        .map(Some)
        .map_err(|_| ReleaseError::Unavailable)
}

fn github_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .https_only(true)
        .redirects(0)
        .timeout_connect(REQUEST_BUDGET)
        .timeout_read(REQUEST_BUDGET)
        .timeout_write(REQUEST_BUDGET)
        .build()
}

async fn run_request<T: Send + 'static>(
    request: impl FnOnce() -> Result<T, ReleaseError> + Send + 'static,
) -> Result<T, ReleaseError> {
    // Include DNS resolution in the UI deadline, even if the system resolver stalls.
    tokio::time::timeout(
        REQUEST_BUDGET,
        tauri::async_runtime::spawn_blocking(request),
    )
    .await
    .map_err(|_| ReleaseError::Unavailable)?
    .map_err(|_| ReleaseError::Unavailable)?
}

fn check_for_update(
    current: &Version,
    agent: &ureq::Agent,
    releases_api: &str,
) -> Result<UpdateCheck, ReleaseError> {
    let deadline = Instant::now() + REQUEST_BUDGET;
    let mut page = 1;
    let mut latest: Option<(Version, GitHubRelease)> = None;

    loop {
        let url = format!("{releases_api}?per_page={PAGE_SIZE}&page={page}");
        let releases: Vec<GitHubRelease> =
            fetch(agent, &url, deadline)?.ok_or(ReleaseError::Unavailable)?;
        let last_page = releases.len() < PAGE_SIZE;
        if let Some(candidate) = latest_windows_release(releases) {
            if latest
                .as_ref()
                .is_none_or(|(version, _)| candidate.0.cmp_precedence(version).is_gt())
            {
                latest = Some(candidate);
            }
        }
        if last_page {
            break;
        }
        page += 1;
    }

    let update_available = latest
        .as_ref()
        .is_some_and(|(version, _)| version.cmp_precedence(current).is_gt());
    Ok(UpdateCheck {
        current_version: current.to_string(),
        latest_release: latest.map(|(version, release)| release.into_app_release(&version)),
        update_available,
    })
}

fn installed_release(current: &Version) -> Result<Option<AppRelease>, ReleaseError> {
    let agent = github_agent();
    let deadline = Instant::now() + REQUEST_BUDGET;
    for tag in [format!("v{current}"), current.to_string()] {
        let url = format!("{API_RELEASES}/tags/{tag}");
        if let Some(release) = fetch::<GitHubRelease>(&agent, &url, deadline)? {
            if !release.draft && parse_tag(&release.tag_name).as_ref() == Some(current) {
                return Ok(Some(release.into_app_release(current)));
            }
        }
    }
    Ok(None)
}

#[tauri::command]
pub(crate) async fn check_for_app_update(app: AppHandle) -> Result<UpdateCheck, ReleaseError> {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return Err(ReleaseError::UnsupportedPlatform);
    }
    let current = app.package_info().version.clone();
    run_request(move || check_for_update(&current, &github_agent(), API_RELEASES)).await
}

#[tauri::command]
pub(crate) async fn get_app_release(app: AppHandle) -> Result<Option<AppRelease>, ReleaseError> {
    let current = app.package_info().version.clone();
    run_request(move || installed_release(&current)).await
}

#[tauri::command]
pub(crate) fn open_app_release(app: AppHandle, tag: Option<String>) -> Result<(), String> {
    let url = release_url(tag.as_deref())?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    fn check_with_local_response(
        current: &Version,
        response: Option<(&'static str, &'static str, &'static str)>,
    ) -> Result<UpdateCheck, ReleaseError> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/releases", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(REQUEST_BUDGET)).unwrap();
            let mut request = BufReader::new(&stream);
            let mut line = String::new();
            request.read_line(&mut line).unwrap();
            assert_eq!(line, "GET /releases?per_page=30&page=1 HTTP/1.1\r\n");
            loop {
                line.clear();
                if request.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            if let Some((status, headers, body)) = response {
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });

        let agent = ureq::AgentBuilder::new().try_proxy_from_env(false).build();
        let result = check_for_update(current, &agent, &endpoint);
        server.join().unwrap();
        result
    }

    fn release(tag: &str, asset_name: &str) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag.to_owned(),
            body: None,
            draft: false,
            prerelease: false,
            assets: vec![GitHubAsset {
                name: asset_name.to_owned(),
                state: "uploaded".to_owned(),
                size: 1,
            }],
        }
    }

    #[test]
    fn chooses_highest_stable_semver_with_the_matching_windows_installer() {
        let mut draft = release("v2.0.0", "Willow_2.0.0_x64-setup.exe");
        draft.draft = true;
        let mut prerelease = release("v3.0.0", "Willow_3.0.0_x64-setup.exe");
        prerelease.prerelease = true;
        let mut incomplete = release("v4.0.0", "Willow_4.0.0_x64-setup.exe");
        incomplete.assets[0].state = "starter".to_owned();

        let found = latest_windows_release([
            release("v0.11.0", "Willow_0.11.0_x64.msi"),
            release("v0.10.0", "Willow_0.10.0_x64-setup.exe"),
            release("0.9.1", "Willow_0.9.1_x64-setup.exe"),
            release("v0.12.0", "Willow_0.12.0_arm64-setup.exe"),
            release("v0.13.0", "Willow_0.10.0_x64-setup.exe"),
            release("v1.0.0-rc.1", "Willow_1.0.0-rc.1_x64-setup.exe"),
            release("unversioned", "Willow_unversioned_x64-setup.exe"),
            draft,
            prerelease,
            incomplete,
        ])
        .unwrap();

        assert_eq!(found.0, Version::new(0, 10, 0));
        assert_eq!(found.1.tag_name, "v0.10.0");
    }

    #[test]
    fn http_check_distinguishes_newer_and_current_versions_after_filtering_assets() {
        const BODY: &str = r#"[
            {
                "tag_name":"v0.6.0",
                "draft":false,
                "prerelease":false,
                "body":"Missing a usable x64 installer.",
                "assets":[
                    {"name":"Willow_0.6.0_x64-setup.exe","state":"uploaded","size":0},
                    {"name":"Willow_0.6.0_arm64-setup.exe","state":"uploaded","size":15}
                ]
            },
            {
                "tag_name":"v0.5.0",
                "draft":false,
                "prerelease":false,
                "body":"Adds the desktop widget.",
                "assets":[{"name":"Willow_0.5.0_x64-setup.exe","state":"uploaded","size":15}]
            }
        ]"#;
        let response = Some(("200 OK", "Content-Type: application/json\r\n", BODY));
        for (current, update_available) in [
            (Version::new(0, 4, 0), true),
            (Version::new(0, 5, 0), false),
        ] {
            let result = check_with_local_response(&current, response).unwrap();
            assert_eq!(result.current_version, current.to_string());
            assert_eq!(result.update_available, update_available);
            let release = result.latest_release.unwrap();
            assert_eq!(release.tag, "v0.5.0");
            assert_eq!(release.notes, "Adds the desktop widget.");
        }
    }

    #[test]
    fn http_check_reports_interrupted_connections_and_rate_limits() {
        let current = Version::new(0, 5, 0);
        assert!(matches!(
            check_with_local_response(&current, None),
            Err(ReleaseError::Unavailable)
        ));

        assert!(matches!(
            check_with_local_response(
                &current,
                Some(("403 Forbidden", "X-RateLimit-Remaining: 0\r\n", "{}")),
            ),
            Err(ReleaseError::RateLimited)
        ));
    }

    #[test]
    fn request_deadline_includes_a_stalled_system_resolver() {
        let result = tauri::async_runtime::block_on(run_request(|| {
            std::thread::sleep(REQUEST_BUDGET + Duration::from_secs(1));
            Ok(())
        }));
        assert!(matches!(result, Err(ReleaseError::Unavailable)));
    }

    #[test]
    fn ignores_build_metadata_for_update_precedence() {
        let published = parse_tag("v0.5.0+release").unwrap();
        let installed = parse_tag("0.5.0+local").unwrap();
        let preview = parse_tag("0.5.0-rc.1").unwrap();
        assert!(published.cmp_precedence(&installed).is_eq());
        assert!(published.cmp_precedence(&preview).is_gt());
    }

    #[test]
    fn opens_only_release_tags_at_the_canonical_repository() {
        assert_eq!(
            release_url(Some("v0.5.0")).unwrap(),
            "https://github.com/hey-pu/Willow/releases/tag/v0.5.0"
        );
        assert_eq!(release_url(None).unwrap(), WEB_RELEASES);
        for tag in [
            "https://example.com",
            "../other",
            "v1.2.3?redirect=external",
        ] {
            assert!(release_url(Some(tag)).is_err());
        }
    }
}
