//! The computer's internet connections (network adapters), so downloads can use
//! a chosen one, or several at once.
//!
//! A download bound to an adapter's address leaves through that adapter. With
//! two or more selected, the engine shares each download's connections among
//! them, which adds their speeds together.

use std::net::{IpAddr, Ipv4Addr};

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NetworkAdapter {
    /// Windows' stable identifier for the adapter (survives IP changes).
    pub id: String,
    /// The name shown in Windows ("Ethernet", "Wi-Fi").
    pub name: String,
    pub description: String,
    /// "ethernet", "wifi", "vpn" or "other"
    pub kind: String,
    pub ipv4: String,
    pub has_gateway: bool,
    /// Link speed in megabits per second, when known.
    pub speed_mbps: Option<u64>,
}

/// Adapters that are not real internet connections (virtual switches, host-only networks...).
fn looks_virtual(description: &str) -> bool {
    let d = description.to_ascii_lowercase();
    ["hyper-v", "vmware", "virtualbox", "vethernet", "loopback", "bluetooth", "host-only", "miniport", "npcap"]
        .iter()
        .any(|w| d.contains(w))
}

fn looks_like_vpn(description: &str, if_type: u32) -> bool {
    let d = description.to_ascii_lowercase();
    if_type == 131 || ["wintun", "wireguard", "tap-windows", "openvpn", "vpn", "tunnel", "tun "].iter().any(|w| d.contains(w))
}

fn kind_of(description: &str, if_type: u32) -> &'static str {
    if looks_like_vpn(description, if_type) {
        "vpn"
    } else if if_type == 71 {
        "wifi"
    } else if if_type == 6 {
        "ethernet"
    } else {
        "other"
    }
}

/// An address that can reach the internet from this adapter (not link-local or loopback).
fn usable(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified() && !ip.is_multicast()
}

/// Internet-capable adapters that are up right now.
pub fn list() -> Vec<NetworkAdapter> {
    #[cfg(windows)]
    {
        windows_list()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
fn windows_list() -> Vec<NetworkAdapter> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
        IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size: u32 = 16 * 1024;
    let mut buffer: Vec<u8> = Vec::new();
    // The needed size can change between calls: retry a few times.
    for _ in 0..4 {
        buffer = vec![0u8; size as usize];
        let rc = unsafe { GetAdaptersAddresses(AF_INET as u32, flags, std::ptr::null(), buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH, &mut size) };
        match rc {
            0 => break,
            111 => continue, // ERROR_BUFFER_OVERFLOW: `size` now holds what is needed
            _ => return Vec::new(),
        }
    }

    let wide = |p: *const u16| -> String {
        if p.is_null() {
            return String::new();
        }
        let mut len = 0;
        unsafe {
            while *p.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
        }
    };
    let narrow = |p: *const u8| -> String {
        if p.is_null() {
            return String::new();
        }
        unsafe { std::ffi::CStr::from_ptr(p as *const std::ffi::c_char).to_string_lossy().to_string() }
    };

    let mut out = Vec::new();
    let mut current = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !current.is_null() {
        let a = unsafe { &*current };
        current = a.Next;
        // IfOperStatusUp == 1
        if a.OperStatus != 1 {
            continue;
        }
        let description = wide(a.Description);
        if looks_virtual(&description) {
            continue;
        }
        let if_type = a.IfType;
        let mut ipv4 = None;
        let mut unicast = a.FirstUnicastAddress;
        while !unicast.is_null() {
            let u = unsafe { &*unicast };
            unicast = u.Next;
            let sa = u.Address.lpSockaddr;
            if !sa.is_null() && unsafe { (*sa).sa_family } == AF_INET {
                let sin = unsafe { &*(sa as *const SOCKADDR_IN) };
                let ip = Ipv4Addr::from(u32::from_be(unsafe { sin.sin_addr.S_un.S_addr }));
                if usable(ip) {
                    ipv4 = Some(ip);
                    break;
                }
            }
        }
        let Some(ip) = ipv4 else { continue };
        let has_gateway = !a.FirstGatewayAddress.is_null();
        // A real connection has a gateway; VPN tunnels often route without one.
        if !has_gateway && !looks_like_vpn(&description, if_type) {
            continue;
        }
        let speed = a.TransmitLinkSpeed;
        out.push(NetworkAdapter {
            id: narrow(a.AdapterName),
            name: wide(a.FriendlyName),
            kind: kind_of(&description, if_type).to_string(),
            description,
            ipv4: ip.to_string(),
            has_gateway,
            speed_mbps: if speed > 0 && speed < u64::MAX / 2 { Some(speed / 1_000_000) } else { None },
        });
    }
    out
}

/// The current addresses of the chosen adapters (those that are still connected).
pub fn addresses_for(ids: &[String]) -> Vec<IpAddr> {
    addresses_from(&list(), ids)
}

fn addresses_from(adapters: &[NetworkAdapter], ids: &[String]) -> Vec<IpAddr> {
    adapters
        .iter()
        .filter(|a| ids.iter().any(|id| id.eq_ignore_ascii_case(&a.id)))
        .filter_map(|a| a.ipv4.parse::<Ipv4Addr>().ok().map(IpAddr::V4))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(id: &str, ip: &str) -> NetworkAdapter {
        NetworkAdapter { id: id.into(), name: id.into(), description: String::new(), kind: "ethernet".into(), ipv4: ip.into(), has_gateway: true, speed_mbps: None }
    }

    #[test]
    fn virtual_and_vpn_adapters_are_told_apart() {
        assert!(looks_virtual("Hyper-V Virtual Ethernet Adapter"));
        assert!(looks_virtual("VMware Virtual Ethernet Adapter for VMnet8"));
        assert!(!looks_virtual("Realtek Gaming 2.5GbE Family Controller"));
        assert!(looks_like_vpn("Wintun Tunnel", 53));
        assert!(looks_like_vpn("Some adapter", 131));
        assert!(!looks_like_vpn("Intel(R) Wi-Fi 6 AX201", 71));
        assert_eq!(kind_of("Intel(R) Wi-Fi 6 AX201", 71), "wifi");
        assert_eq!(kind_of("Realtek PCIe GbE", 6), "ethernet");
        assert_eq!(kind_of("WireGuard Tunnel", 53), "vpn");
    }

    #[test]
    fn only_routable_addresses_count() {
        assert!(usable("192.168.0.5".parse().unwrap()));
        assert!(usable("10.1.2.3".parse().unwrap()));
        assert!(!usable("169.254.10.1".parse().unwrap()), "link-local: no network configured");
        assert!(!usable("127.0.0.1".parse().unwrap()));
        assert!(!usable("0.0.0.0".parse().unwrap()));
    }

    #[test]
    fn chosen_adapters_map_to_their_current_addresses() {
        let all = vec![adapter("{A}", "192.168.0.5"), adapter("{B}", "10.0.0.8")];
        let ips = addresses_from(&all, &["{b}".to_string(), "{GONE}".to_string()]);
        assert_eq!(ips, vec!["10.0.0.8".parse::<IpAddr>().unwrap()], "ids match case-insensitively; adapters that left are skipped");
        assert!(addresses_from(&all, &[]).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn this_computers_adapters_can_be_listed() {
        // Whatever is connected here, listing must not crash and must only return usable addresses.
        for a in list() {
            println!("adapter: {a:?}");
            let ip: Ipv4Addr = a.ipv4.parse().unwrap();
            assert!(usable(ip), "{a:?}");
            assert!(!a.id.is_empty() && !a.name.is_empty(), "{a:?}");
        }
    }
}
