use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Client for short control requests (probing, page scans). `timeout` bounds
/// the *whole* request, which is exactly what we want for small responses.
pub fn create_http_client(timeout: Duration) -> reqwest::Client {
    create_http_client_with_proxy(timeout, None, None)
}

pub fn create_http_client_with_proxy(
    timeout: Duration,
    proxy_mode: Option<&str>,
    custom_proxy_url: Option<&str>,
) -> reqwest::Client {
    build_client(
        reqwest::Client::builder().timeout(timeout),
        proxy_mode,
        custom_proxy_url,
    )
}

/// Client for bulk transfers. There is deliberately **no total-request
/// timeout** (it would also cap how long the body may stream and kill every
/// download slower than `size / timeout`). Instead: a connect timeout, and an
/// idle timeout that resets on every successful read, so only a stalled
/// connection is cut off.
pub fn create_download_client_with_proxy(
    idle_timeout: Duration,
    proxy_mode: Option<&str>,
    custom_proxy_url: Option<&str>,
) -> reqwest::Client {
    create_download_client_from(idle_timeout, proxy_mode, custom_proxy_url, None)
}

/// Like [`create_download_client_with_proxy`], but every connection leaves through
/// the network adapter that owns `local` (when given).
pub fn create_download_client_from(
    idle_timeout: Duration,
    proxy_mode: Option<&str>,
    custom_proxy_url: Option<&str>,
    local: Option<IpAddr>,
) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(idle_timeout)
        .tcp_keepalive(Duration::from_secs(30));
    if let Some(ip) = local {
        builder = builder.local_address(ip);
    }
    build_client(builder, proxy_mode, custom_proxy_url)
}

/// True when following a redirect would move from encrypted to unencrypted transport.
pub fn is_downgrade(previous_scheme: &str, next_scheme: &str) -> bool {
    previous_scheme == "https" && next_scheme == "http"
}

/// Redirect handling: follow up to 10 hops, but never silently fall from HTTPS
/// back to plain HTTP. That would let anyone on the network swap the file we
/// are about to save.
fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            return attempt.error("Too many redirects");
        }
        let downgrade = attempt
            .previous()
            .last()
            .map(|prev| is_downgrade(prev.scheme(), attempt.url().scheme()))
            .unwrap_or(false);
        if downgrade {
            attempt.error("Refusing to follow a redirect from HTTPS to unencrypted HTTP")
        } else {
            attempt.follow()
        }
    })
}

fn build_client(
    builder: reqwest::ClientBuilder,
    proxy_mode: Option<&str>,
    custom_proxy_url: Option<&str>,
) -> reqwest::Client {
    let mut builder = builder.pool_max_idle_per_host(32).redirect(redirect_policy());

    let mode = proxy_mode.unwrap_or("system");

    match mode {
        "direct" => {
            // Explicitly disable any proxying
            builder = builder.no_proxy();
        }
        "custom" => {
            if let Some(custom_url) = custom_proxy_url {
                let trimmed = custom_url.trim();
                if !trimmed.is_empty() {
                    let full_url = if trimmed.starts_with("http://")
                        || trimmed.starts_with("https://")
                        || trimmed.starts_with("socks5://")
                    {
                        trimmed.to_string()
                    } else {
                        format!("http://{}", trimmed)
                    };
                    if let Ok(proxy) = reqwest::Proxy::all(&full_url) {
                        builder = builder.proxy(proxy);
                    }
                }
            }
        }
        _ => {
            // "system" mode: On Windows, read system proxy settings (v2ray, clash, corporate VPN)
            #[cfg(target_os = "windows")]
            {
                if let Ok(hkcu) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
                    .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
                {
                    let proxy_enable: Result<u32, _> = hkcu.get_value("ProxyEnable");
                    if let Ok(1) = proxy_enable {
                        if let Ok(proxy_server) = hkcu.get_value::<String, _>("ProxyServer") {
                            let server_clean = proxy_server.trim();
                            if !server_clean.is_empty() {
                                let proxy_target = if let Some(http_part) =
                                    server_clean.split(';').find(|s| s.starts_with("http="))
                                {
                                    http_part.trim_start_matches("http=").to_string()
                                } else {
                                    server_clean.to_string()
                                };

                                let proxy_url = if proxy_target.starts_with("http://")
                                    || proxy_target.starts_with("socks5://")
                                {
                                    proxy_target
                                } else {
                                    format!("http://{}", proxy_target)
                                };

                                if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
                                    builder = builder.proxy(proxy);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    builder.build().unwrap_or_default()
}

/// How long a cached client that follows the *system* proxy is reused. The
/// Windows proxy setting is only read when a client is built, so this keeps
/// VPN/proxy toggles taking effect within a short time.
const SYSTEM_PROXY_TTL: Duration = Duration::from_secs(30);

#[derive(Hash, PartialEq, Eq, Clone)]
struct PoolKey {
    bulk: bool,
    local: Option<IpAddr>,
    mode: String,
    custom: String,
}

/// Shares HTTP clients between downloads and probes.
///
/// A `reqwest::Client` owns the connection pool, TLS sessions and DNS cache, so
/// building one per download (as before) threw all of that away and re-read the
/// Windows registry every time. Clients are keyed by their proxy configuration.
#[derive(Default)]
pub struct ClientPool {
    clients: Mutex<HashMap<PoolKey, (Instant, reqwest::Client)>>,
}

impl ClientPool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Client for bulk transfers (no total timeout, idle-read timeout only).
    pub fn download(&self, proxy_mode: Option<&str>, custom_proxy_url: Option<&str>) -> reqwest::Client {
        self.get(true, proxy_mode, custom_proxy_url, None)
    }

    /// Client for bulk transfers that leaves through the adapter owning `local`.
    pub fn download_from(&self, proxy_mode: Option<&str>, custom_proxy_url: Option<&str>, local: Option<IpAddr>) -> reqwest::Client {
        self.get(true, proxy_mode, custom_proxy_url, local)
    }

    /// Client for short control requests (probing).
    pub fn probe(&self, proxy_mode: Option<&str>, custom_proxy_url: Option<&str>) -> reqwest::Client {
        self.get(false, proxy_mode, custom_proxy_url, None)
    }

    fn get(&self, bulk: bool, proxy_mode: Option<&str>, custom_proxy_url: Option<&str>, local: Option<IpAddr>) -> reqwest::Client {
        let mode = proxy_mode.unwrap_or("system");
        let key = PoolKey {
            bulk,
            local,
            mode: mode.to_string(),
            custom: if mode == "custom" { custom_proxy_url.unwrap_or("").trim().to_string() } else { String::new() },
        };

        let mut clients = self.clients.lock();
        if let Some((built, client)) = clients.get(&key) {
            if mode != "system" || built.elapsed() < SYSTEM_PROXY_TTL {
                return client.clone();
            }
        }
        let client = if bulk {
            create_download_client_from(Duration::from_secs(30), Some(mode), custom_proxy_url, local)
        } else {
            create_http_client_with_proxy(Duration::from_secs(15), Some(mode), custom_proxy_url)
        };
        clients.insert(key, (Instant::now(), client.clone()));
        client
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_client_bound_to_an_address_connects_from_it() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut sock, peer) = listener.accept().await.unwrap();
            let mut buf = [0u8; 512];
            let _ = sock.read(&mut buf).await;
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").await;
            peer.ip()
        });
        let from: IpAddr = "127.0.0.1".parse().unwrap();
        let client = create_download_client_from(Duration::from_secs(5), Some("direct"), None, Some(from));
        let body = client.get(format!("http://127.0.0.1:{port}/")).send().await.unwrap().text().await.unwrap();
        assert_eq!(body, "ok");
        assert_eq!(server.await.unwrap(), from, "the server saw the connection come from the chosen address");
    }

    #[test]
    fn only_https_to_http_is_a_downgrade() {
        assert!(is_downgrade("https", "http"));
        assert!(!is_downgrade("http", "https"));
        assert!(!is_downgrade("http", "http"));
        assert!(!is_downgrade("https", "https"));
    }
}
