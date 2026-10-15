use std::time::Duration;

pub fn create_http_client(timeout: Duration) -> reqwest::Client {
    create_http_client_with_proxy(timeout, None, None)
}

pub fn create_http_client_with_proxy(
    timeout: Duration,
    proxy_mode: Option<&str>,
    custom_proxy_url: Option<&str>,
) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(timeout)
        .pool_max_idle_per_host(10);

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
