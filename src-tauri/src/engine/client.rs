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

/// The custom proxy's login, set from the settings (shared by every client built).
static PROXY_AUTH: std::sync::LazyLock<parking_lot::RwLock<(String, String)>> =
    std::sync::LazyLock::new(|| parking_lot::RwLock::new((String::new(), String::new())));
/// Bumped whenever the login changes, so cached clients are rebuilt with it.
static PROXY_AUTH_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn set_proxy_auth(username: &str, password: &str) {
    let mut auth = PROXY_AUTH.write();
    if auth.0 != username || auth.1 != password {
        *auth = (username.to_string(), password.to_string());
        PROXY_AUTH_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// A proxy address as typed (`127.0.0.1:1080`, `socks5://host:port`, `http://...`) turned
/// into what the HTTP client needs. SOCKS5 resolves names through the proxy (`socks5h`),
/// so blocked or poisoned DNS on this computer doesn't matter. The login is added unless
/// the address already carries one.
pub fn proxy_url_with_auth(raw: &str, username: &str, password: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let with_scheme = if lower.starts_with("socks5h://") || lower.starts_with("http://") || lower.starts_with("https://") {
        trimmed.to_string()
    } else if let Some(rest) = lower.strip_prefix("socks5://").or_else(|| lower.strip_prefix("socks://")) {
        format!("socks5h://{}", &trimmed[trimmed.len() - rest.len()..])
    } else {
        format!("http://{trimmed}")
    };
    let mut url = url::Url::parse(&with_scheme).ok()?;
    if url.username().is_empty() && !username.is_empty() {
        let _ = url.set_username(username);
        let _ = url.set_password(if password.is_empty() { None } else { Some(password) });
    }
    Some(url.to_string())
}

fn build_client(
    builder: reqwest::ClientBuilder,
    proxy_mode: Option<&str>,
    custom_proxy_url: Option<&str>,
) -> reqwest::Client {
    let (user, pass) = PROXY_AUTH.read().clone();
    build_client_with_auth(builder, proxy_mode, custom_proxy_url, &user, &pass)
}

fn build_client_with_auth(
    builder: reqwest::ClientBuilder,
    proxy_mode: Option<&str>,
    custom_proxy_url: Option<&str>,
    user: &str,
    pass: &str,
) -> reqwest::Client {
    let mut builder = builder.pool_max_idle_per_host(32).redirect(redirect_policy());

    let mode = proxy_mode.unwrap_or("system");

    match mode {
        "direct" => {
            // Explicitly disable any proxying
            builder = builder.no_proxy();
        }
        "custom" => {
            if let Some(raw) = custom_proxy_url.filter(|u| !u.trim().is_empty()) {
                match proxy_url_with_auth(raw, user, pass).and_then(|u| reqwest::Proxy::all(&u).ok()) {
                    Some(proxy) => builder = builder.proxy(proxy),
                    // A proxy was asked for but its address is unusable: never fall back to a
                    // direct connection, send everything to an address that refuses it.
                    None => builder = builder.proxy(reqwest::Proxy::all("http://127.0.0.1:9").expect("static proxy URL")),
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
                                // "host:port", or per protocol: "http=host:port;socks=host:port"
                                let proxy_target = if let Some(http_part) =
                                    server_clean.split(';').find(|s| s.starts_with("http="))
                                {
                                    http_part.trim_start_matches("http=").to_string()
                                } else if let Some(socks) = server_clean.split(';').find(|s| s.starts_with("socks=")) {
                                    format!("socks5h://{}", socks.trim_start_matches("socks="))
                                } else {
                                    server_clean.to_string()
                                };

                                let proxy_url = if proxy_target.starts_with("http://")
                                    || proxy_target.starts_with("socks5://")
                                    || proxy_target.starts_with("socks5h://")
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
    auth_gen: u64,
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
            auth_gen: PROXY_AUTH_GEN.load(std::sync::atomic::Ordering::Relaxed),
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

/// Result of trying the proxy settings against a public address.
#[derive(serde::Serialize)]
pub struct ProxyTest {
    pub ok: bool,
    pub millis: u64,
    /// The address websites see (when the test page tells us).
    pub exit_ip: Option<String>,
    pub country: Option<String>,
    pub error: Option<String>,
}

/// Fetches a tiny page through the given proxy settings and reports what happened.
pub async fn test_proxy(mode: &str, url: Option<&str>, username: &str, password: &str) -> ProxyTest {
    let client = build_client_with_auth(
        reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(15)),
        Some(mode),
        url,
        username,
        password,
    );
    let started = Instant::now();
    match client.get("https://cloudflare.com/cdn-cgi/trace").send().await {
        Ok(resp) if resp.status().is_success() => {
            let body = resp.text().await.unwrap_or_default();
            let field = |k: &str| body.lines().find_map(|l| l.strip_prefix(&format!("{k}=")).map(str::to_string));
            ProxyTest { ok: true, millis: started.elapsed().as_millis() as u64, exit_ip: field("ip"), country: field("loc"), error: None }
        }
        Ok(resp) => ProxyTest {
            ok: false,
            millis: started.elapsed().as_millis() as u64,
            exit_ip: None,
            country: None,
            error: Some(if resp.status().as_u16() == 407 { "The proxy wants a different username or password".to_string() } else { format!("HTTP {}", resp.status()) }),
        },
        Err(e) => {
            let mut msg = e.to_string();
            let mut src = std::error::Error::source(&e);
            while let Some(inner) = src {
                msg = inner.to_string();
                src = inner.source();
            }
            ProxyTest { ok: false, millis: started.elapsed().as_millis() as u64, exit_ip: None, country: None, error: Some(msg) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal SOCKS5 server that insists on a username and password, in front of a web server.
    #[tokio::test]
    async fn downloads_go_through_a_socks5_proxy_with_a_login() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};
        let web = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let web_port = web.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = web.accept().await {
                tokio::spawn(async move {
                    let mut b = [0u8; 1024];
                    let _ = s.read(&mut b).await;
                    let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nviasock").await;
                });
            }
        });
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = proxy.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut c, _)) = proxy.accept().await {
                tokio::spawn(async move {
                    let mut h = [0u8; 2];
                    c.read_exact(&mut h).await?;
                    let mut methods = vec![0u8; h[1] as usize];
                    c.read_exact(&mut methods).await?;
                    if !methods.contains(&2) {
                        c.write_all(&[5, 0xff]).await?;
                        return Ok::<_, std::io::Error>(());
                    }
                    c.write_all(&[5, 2]).await?;
                    let mut v = [0u8; 2];
                    c.read_exact(&mut v).await?;
                    let mut user = vec![0u8; v[1] as usize];
                    c.read_exact(&mut user).await?;
                    let mut pl = [0u8; 1];
                    c.read_exact(&mut pl).await?;
                    let mut pass = vec![0u8; pl[0] as usize];
                    c.read_exact(&mut pass).await?;
                    let ok = user == b"alice" && pass == b"p@ss";
                    c.write_all(&[1, if ok { 0 } else { 1 }]).await?;
                    if !ok {
                        return Ok(());
                    }
                    let mut req = [0u8; 4];
                    c.read_exact(&mut req).await?;
                    let port_of = |b: [u8; 2]| u16::from_be_bytes(b);
                    let host = match req[3] {
                        3 => {
                            let mut l = [0u8; 1];
                            c.read_exact(&mut l).await?;
                            let mut name = vec![0u8; l[0] as usize];
                            c.read_exact(&mut name).await?;
                            String::from_utf8_lossy(&name).to_string()
                        }
                        _ => {
                            let mut ip = [0u8; 4];
                            c.read_exact(&mut ip).await?;
                            std::net::Ipv4Addr::from(ip).to_string()
                        }
                    };
                    let mut pb = [0u8; 2];
                    c.read_exact(&mut pb).await?;
                    let mut upstream = TcpStream::connect((host.as_str(), port_of(pb))).await?;
                    c.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
                    tokio::io::copy_bidirectional(&mut c, &mut upstream).await?;
                    Ok(())
                });
            }
        });
        let proxy_url = format!("socks5://127.0.0.1:{proxy_port}");
        let target = format!("http://localhost:{web_port}/");

        let good = build_client_with_auth(reqwest::Client::builder(), Some("custom"), Some(&proxy_url), "alice", "p@ss");
        assert_eq!(good.get(&target).send().await.unwrap().text().await.unwrap(), "viasock");

        let wrong = build_client_with_auth(reqwest::Client::builder(), Some("custom"), Some(&proxy_url), "alice", "nope");
        assert!(wrong.get(&target).send().await.is_err(), "a wrong login must not get through");

        // A broken proxy address must never fall back to a direct connection.
        let broken = build_client_with_auth(reqwest::Client::builder(), Some("custom"), Some("socks5://[::bad"), "", "");
        assert!(broken.get(&target).send().await.is_err());
    }

    #[test]
    fn proxy_addresses_are_normalised_and_get_the_login() {
        assert_eq!(proxy_url_with_auth("127.0.0.1:10808", "", "").unwrap(), "http://127.0.0.1:10808/");
        assert_eq!(proxy_url_with_auth("socks5://127.0.0.1:1080", "", "").unwrap(), "socks5h://127.0.0.1:1080");
        assert_eq!(proxy_url_with_auth("SOCKS://10.0.0.1:1080", "", "").unwrap(), "socks5h://10.0.0.1:1080");
        assert_eq!(proxy_url_with_auth("socks5://127.0.0.1:1080", "me", "p@ss:word").unwrap(), "socks5h://me:p%40ss%3Aword@127.0.0.1:1080");
        assert_eq!(proxy_url_with_auth("http://u:x@h:8080", "me", "y").unwrap(), "http://u:x@h:8080/", "a login in the address wins");
        assert!(proxy_url_with_auth("   ", "me", "y").is_none());
    }

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
