use anyhow::{Context, Result};
use reqwest::blocking::{Client, ClientBuilder};
use reqwest::Url;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use super::skill_store::SkillStore;

pub const GITHUB_PROXY_URL_KEY: &str = "github_proxy_url";
pub const DEFAULT_GITHUB_PROXY_URL: &str = "http://127.0.0.1:7890";
const DEFAULT_GITHUB_PROXY_HOST: &str = "127.0.0.1";
pub const DEFAULT_GITHUB_PROXY_PORT: u16 = 7890;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubProxyConfig {
    pub enabled: bool,
    pub port: u16,
    pub url: String,
    pub auto_detected: bool,
}

pub fn get_github_proxy_url(store: &SkillStore) -> Result<String> {
    Ok(get_github_proxy_config(store)?.url)
}

pub fn get_github_proxy_config(store: &SkillStore) -> Result<GithubProxyConfig> {
    match store.get_setting(GITHUB_PROXY_URL_KEY)? {
        Some(value) => match validated_proxy_url(&value) {
            Ok(Some(url)) => proxy_config_from_validated_url(&url, false),
            Ok(None) => Ok(disabled_proxy_config(false)),
            Err(_) => {
                // A legacy build allowed arbitrary proxy URLs. Never use or
                // echo an unsafe saved value; persist the disabled state.
                log::warn!("unsafe saved GitHub proxy configuration was disabled");
                store.set_setting(GITHUB_PROXY_URL_KEY, "")?;
                Ok(disabled_proxy_config(false))
            }
        },
        None => {
            let url = auto_detect_github_proxy_url();
            match validated_proxy_url(&url)? {
                Some(url) => proxy_config_from_validated_url(&url, true),
                None => Ok(disabled_proxy_config(false)),
            }
        }
    }
}

pub fn set_github_proxy_config(
    store: &SkillStore,
    enabled: bool,
    port: u16,
) -> Result<GithubProxyConfig> {
    let normalized_port = if port == 0 {
        DEFAULT_GITHUB_PROXY_PORT
    } else {
        port
    };
    let url = if enabled {
        format!("http://{}:{}", DEFAULT_GITHUB_PROXY_HOST, normalized_port)
    } else {
        String::new()
    };
    store.set_setting(GITHUB_PROXY_URL_KEY, &url)?;
    match validated_proxy_url(&url)? {
        Some(url) => proxy_config_from_validated_url(&url, false),
        None => Ok(disabled_proxy_config(false)),
    }
}

pub fn auto_detect_github_proxy_url() -> String {
    if local_tcp_port_is_open(
        DEFAULT_GITHUB_PROXY_HOST,
        DEFAULT_GITHUB_PROXY_PORT,
        Duration::from_millis(200),
    ) {
        DEFAULT_GITHUB_PROXY_URL.to_string()
    } else {
        String::new()
    }
}

pub fn app_http_client(proxy_url: &str, timeout_secs: Option<u64>) -> Result<Client> {
    let mut builder = ClientBuilder::new();
    if let Some(secs) = timeout_secs {
        builder = builder.timeout(std::time::Duration::from_secs(secs));
    }
    if let Some(proxy_url) = validated_proxy_url(proxy_url)? {
        builder = builder
            .proxy(reqwest::Proxy::all(&proxy_url).context("invalid local proxy configuration")?);
    }
    builder.build().context("build HTTP client")
}

pub fn github_http_client(proxy_url: &str, timeout_secs: Option<u64>) -> Result<Client> {
    app_http_client(proxy_url, timeout_secs)
}

fn disabled_proxy_config(auto_detected: bool) -> GithubProxyConfig {
    GithubProxyConfig {
        enabled: false,
        port: DEFAULT_GITHUB_PROXY_PORT,
        url: String::new(),
        auto_detected,
    }
}

fn proxy_config_from_validated_url(url: &str, auto_detected: bool) -> Result<GithubProxyConfig> {
    let port = proxy_port_from_url(url)
        .ok_or_else(|| anyhow::anyhow!("INVALID_PROXY_CONFIG|Local proxy port is invalid"))?;
    Ok(GithubProxyConfig {
        enabled: true,
        port,
        url: url.to_string(),
        auto_detected,
    })
}

fn proxy_port_from_url(url: &str) -> Option<u16> {
    Url::parse(url).ok()?.port_or_known_default()
}

pub(crate) fn validate_proxy_url(proxy_url: &str) -> Result<()> {
    validated_proxy_url(proxy_url).map(|_| ())
}

fn validated_proxy_url(proxy_url: &str) -> Result<Option<String>> {
    let value = proxy_url.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().any(char::is_control) {
        anyhow::bail!("INVALID_PROXY_CONFIG|Local proxy configuration is malformed");
    }

    let url = Url::parse(value).map_err(|_| {
        anyhow::anyhow!("INVALID_PROXY_CONFIG|Local proxy configuration is invalid")
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        anyhow::bail!("INVALID_PROXY_CONFIG|Local proxy must use HTTP or HTTPS");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("INVALID_PROXY_CONFIG|Local proxy host is missing"))?;
    if !host.eq_ignore_ascii_case("localhost")
        && host != "127.0.0.1"
        && host != "::1"
        && host != "[::1]"
    {
        anyhow::bail!("UNSAFE_PROXY_CONFIG|Proxy host must be local");
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || proxy_authority_has_userinfo(value)
        || url.query().is_some()
        || url.fragment().is_some()
    {
        anyhow::bail!(
            "UNSAFE_PROXY_CONFIG|Local proxy must not contain credentials, query, or fragment"
        );
    }
    if !matches!(url.path(), "" | "/") {
        anyhow::bail!("UNSAFE_PROXY_CONFIG|Local proxy path must be empty");
    }
    if url.port_or_known_default().is_none() {
        anyhow::bail!("INVALID_PROXY_CONFIG|Local proxy port is invalid");
    }

    Ok(Some(url.as_str().trim_end_matches('/').to_string()))
}

fn proxy_authority_has_userinfo(value: &str) -> bool {
    let Some((_, remainder)) = value.split_once("://") else {
        return false;
    };
    remainder
        .split(['/', '?', '#'])
        .next()
        .is_some_and(|authority| authority.contains('@'))
}

fn local_tcp_port_is_open(host: &str, port: u16, timeout: Duration) -> bool {
    let Ok(addr) = format!("{}:{}", host, port).parse::<SocketAddr>() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, timeout).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill_store::SkillStore;
    use std::net::TcpListener;

    #[test]
    fn empty_saved_github_proxy_disables_proxy() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillStore::new(dir.path().join("db.sqlite"));
        store.ensure_schema().unwrap();

        store.set_setting(GITHUB_PROXY_URL_KEY, "  ").unwrap();

        assert_eq!(get_github_proxy_url(&store).unwrap(), "");
    }

    #[test]
    fn proxy_config_disable_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillStore::new(dir.path().join("db.sqlite"));
        store.ensure_schema().unwrap();

        let saved = set_github_proxy_config(&store, false, 7890).unwrap();

        assert!(!saved.enabled);
        assert_eq!(saved.port, DEFAULT_GITHUB_PROXY_PORT);
        assert_eq!(saved.url, "");
        assert!(!get_github_proxy_config(&store).unwrap().enabled);
    }

    #[test]
    fn proxy_config_uses_localhost_port() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillStore::new(dir.path().join("db.sqlite"));
        store.ensure_schema().unwrap();

        let saved = set_github_proxy_config(&store, true, 7897).unwrap();

        assert!(saved.enabled);
        assert_eq!(saved.port, 7897);
        assert_eq!(saved.url, "http://127.0.0.1:7897");
        assert_eq!(get_github_proxy_url(&store).unwrap(), saved.url);
    }

    #[test]
    fn valid_legacy_local_proxy_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillStore::new(dir.path().join("db.sqlite"));
        store.ensure_schema().unwrap();

        store
            .set_setting(GITHUB_PROXY_URL_KEY, " http://localhost:7890/ ")
            .unwrap();

        let config = get_github_proxy_config(&store).unwrap();
        assert!(config.enabled);
        assert_eq!(config.port, 7890);
        assert_eq!(config.url, "http://localhost:7890");
    }

    #[test]
    fn unsafe_saved_proxy_is_persistently_disabled_without_echoing_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillStore::new(dir.path().join("db.sqlite"));
        store.ensure_schema().unwrap();

        for unsafe_url in [
            "http://attacker.example:7890",
            "http://user:super-secret@127.0.0.1:7890",
            "http://127.0.0.1:7890?token=super-secret",
            "http://127.0.0.1:7890#super-secret",
            "http://127.0.0.1:7890/proxy",
            "socks5://127.0.0.1:7890",
        ] {
            store.set_setting(GITHUB_PROXY_URL_KEY, unsafe_url).unwrap();
            let config = get_github_proxy_config(&store).unwrap();
            assert!(!config.enabled);
            assert_eq!(config.url, "");
            assert_eq!(
                store.get_setting(GITHUB_PROXY_URL_KEY).unwrap().as_deref(),
                Some("")
            );
        }
    }

    #[test]
    fn http_client_rejects_unsafe_proxy_without_secret_in_error() {
        let err = match app_http_client("http://user:super-secret@127.0.0.1:7890", Some(1)) {
            Ok(_) => panic!("expected unsafe proxy rejection"),
            Err(err) => err,
        };
        let message = format!("{err:#}");
        assert!(message.contains("UNSAFE_PROXY_CONFIG"));
        assert!(!message.contains("super-secret"));
    }

    #[test]
    fn strict_proxy_validator_allows_only_local_http_endpoints() {
        for valid in [
            "http://localhost:7890",
            "https://127.0.0.1:7890/",
            "http://[::1]:7890",
        ] {
            validate_proxy_url(valid).unwrap();
        }

        for invalid in [
            "ftp://127.0.0.1:7890",
            "http://192.168.1.10:7890",
            "http://localhost:7890/path",
            "http://localhost:7890?q=1",
            "http://localhost:7890#fragment",
            "http://user@localhost:7890",
        ] {
            assert!(validate_proxy_url(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn local_tcp_port_detector_sees_open_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        assert!(local_tcp_port_is_open(
            "127.0.0.1",
            port,
            Duration::from_millis(200)
        ));
    }
}
