//! Browsers on this computer and whether Nexdrop's extension is working in them.
//!
//! The extension says hello to the app now and then (see `/hello` in the
//! bridge), naming its browser. That is how the app knows a browser is
//! connected, and it is remembered across restarts so a browser that was
//! connected once is not nagged about later.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// A browser counts as connected while its extension has been heard from this recently.
const CONNECTED_WINDOW_SECS: i64 = 12 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Seen {
    version: String,
    first_seen: i64,
    last_seen: i64,
}

fn seen_store() -> &'static Mutex<HashMap<String, Seen>> {
    static STORE: OnceLock<Mutex<HashMap<String, Seen>>> = OnceLock::new();
    STORE.get_or_init(|| {
        let loaded = std::fs::read_to_string(seen_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Mutex::new(loaded)
    })
}

fn seen_path() -> PathBuf {
    crate::util::data_dir().join("browsers_seen.json")
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The extension in `browser` just checked in.
pub fn note_hello(browser: &str, version: &str) {
    let browser = browser.trim().to_ascii_lowercase();
    if browser.is_empty() || browser.len() > 20 || !browser.chars().all(|c| c.is_ascii_alphanumeric()) {
        return;
    }
    let t = now();
    let mut store = seen_store().lock();
    let entry = store.entry(browser).or_insert_with(|| Seen { first_seen: t, ..Default::default() });
    let changed = entry.version != version || t - entry.last_seen > 60;
    entry.version = version.chars().take(20).collect();
    entry.last_seen = t;
    if changed {
        let _ = std::fs::create_dir_all(crate::util::data_dir());
        let _ = std::fs::write(seen_path(), serde_json::to_string(&*store).unwrap_or_default());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Chromium,
    Firefox,
}

#[derive(Debug, Clone)]
pub struct Installed {
    pub id: &'static str,
    pub name: &'static str,
    pub family: Family,
    pub exe: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowserInfo {
    pub id: String,
    pub name: String,
    /// "chromium" or "firefox"
    pub family: String,
    /// Found on this computer.
    pub installed: bool,
    /// The extension has said hello recently.
    pub connected: bool,
    /// It has said hello at some point (so it is installed, even if the browser is closed now).
    pub ever_connected: bool,
    pub last_seen_secs_ago: Option<i64>,
    pub version: Option<String>,
}

// ───────────────────────────── finding browsers ─────────────────────────────

struct Known {
    id: &'static str,
    name: &'static str,
    family: Family,
    exe: &'static str,
    /// Install locations relative to the Program Files / Local AppData roots.
    rel: &'static [&'static str],
}

const KNOWN: [Known; 4] = [
    Known { id: "chrome", name: "Google Chrome", family: Family::Chromium, exe: "chrome.exe", rel: &[r"Google\Chrome\Application\chrome.exe"] },
    Known { id: "edge", name: "Microsoft Edge", family: Family::Chromium, exe: "msedge.exe", rel: &[r"Microsoft\Edge\Application\msedge.exe"] },
    Known { id: "brave", name: "Brave", family: Family::Chromium, exe: "brave.exe", rel: &[r"BraveSoftware\Brave-Browser\Application\brave.exe"] },
    Known { id: "firefox", name: "Mozilla Firefox", family: Family::Firefox, exe: "firefox.exe", rel: &[r"Mozilla Firefox\firefox.exe"] },
];

fn registered_path(exe: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        use winreg::RegKey;
        let sub = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
        for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            if let Ok(key) = RegKey::predef(hive).open_subkey(&sub) {
                if let Ok(path) = key.get_value::<String, _>("") {
                    let p = PathBuf::from(path.trim_matches('"'));
                    if p.exists() {
                        return Some(p);
                    }
                }
            }
        }
    }
    let _ = exe;
    None
}

/// Browsers installed on this computer that Nexdrop has an extension for.
pub fn detect() -> Vec<Installed> {
    let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .collect();
    KNOWN
        .iter()
        .filter_map(|k| {
            let found = registered_path(k.exe).or_else(|| {
                roots
                    .iter()
                    .flat_map(|root| k.rel.iter().map(move |rel| root.join(rel)))
                    .find(|p| p.exists())
            })?;
            Some(Installed { id: k.id, name: k.name, family: k.family, exe: found })
        })
        .collect()
}

/// Everything the interface needs: each browser, and whether its extension is working.
pub fn status() -> Vec<BrowserInfo> {
    status_from(&detect(), &seen_store().lock(), now())
}

fn status_from(installed: &[Installed], seen: &HashMap<String, Seen>, now: i64) -> Vec<BrowserInfo> {
    let mut out: Vec<BrowserInfo> = Vec::new();
    for k in KNOWN.iter() {
        let is_installed = installed.iter().any(|i| i.id == k.id);
        let s = seen.get(k.id);
        if !is_installed && s.is_none() {
            continue;
        }
        let ago = s.map(|s| (now - s.last_seen).max(0));
        out.push(BrowserInfo {
            id: k.id.to_string(),
            name: k.name.to_string(),
            family: if k.family == Family::Firefox { "firefox" } else { "chromium" }.to_string(),
            installed: is_installed,
            connected: ago.map(|a| a <= CONNECTED_WINDOW_SECS).unwrap_or(false),
            ever_connected: s.is_some(),
            last_seen_secs_ago: ago,
            version: s.map(|s| s.version.clone()),
        });
    }
    out
}

// ───────────────────────────── setting up ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct SetupOutcome {
    /// "unpacked" (Chrome/Edge/Brave), "xpi" (signed Firefox add-on) or "temporary" (unsigned Firefox add-on).
    pub mode: &'static str,
    /// Folder to load (unpacked / temporary).
    pub folder: Option<String>,
    /// File Firefox was asked to install.
    pub file: Option<String>,
}

fn chromium_scheme(id: &str) -> &'static str {
    match id {
        "edge" => "edge",
        "brave" => "brave",
        _ => "chrome",
    }
}

/// Opens what the user needs in `id`'s browser to finish installing the extension.
pub fn setup(id: &str, extension_dir: &Path, signed_xpi: Option<&Path>) -> Result<SetupOutcome, String> {
    let browser = detect().into_iter().find(|b| b.id == id).ok_or_else(|| "That browser wasn't found on this computer".to_string())?;
    let folder = extension_dir.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    match browser.family {
        Family::Chromium => {
            crate::process::command(&browser.exe)
                .arg(format!("{}://extensions", chromium_scheme(id)))
                .spawn()
                .map_err(|e| format!("Couldn't open {}: {e}", browser.name))?;
            Ok(SetupOutcome { mode: "unpacked", folder: Some(folder), file: None })
        }
        Family::Firefox => match signed_xpi.filter(|p| p.exists()) {
            Some(xpi) => {
                let file = xpi.to_string_lossy().trim_start_matches(r"\\?\").to_string();
                crate::process::command(&browser.exe)
                    .arg(&file)
                    .spawn()
                    .map_err(|e| format!("Couldn't open {}: {e}", browser.name))?;
                Ok(SetupOutcome { mode: "xpi", folder: None, file: Some(file) })
            }
            None => {
                crate::process::command(&browser.exe)
                    .arg("about:debugging#/runtime/this-firefox")
                    .spawn()
                    .map_err(|e| format!("Couldn't open {}: {e}", browser.name))?;
                Ok(SetupOutcome { mode: "temporary", folder: Some(folder), file: None })
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(id: &'static str) -> Installed {
        let k = KNOWN.iter().find(|k| k.id == id).unwrap();
        Installed { id: k.id, name: k.name, family: k.family, exe: PathBuf::from("x.exe") }
    }

    #[test]
    fn only_browsers_that_exist_or_have_been_seen_are_listed() {
        let mut seen = HashMap::new();
        seen.insert("brave".to_string(), Seen { version: "1.2.0".into(), first_seen: 0, last_seen: 995 });
        let list = status_from(&[installed("edge")], &seen, 1000);
        let ids: Vec<&str> = list.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["edge", "brave"], "Edge is installed, Brave was heard from; Chrome and Firefox are neither");
    }

    #[test]
    fn connected_means_heard_from_recently() {
        let mut seen = HashMap::new();
        seen.insert("edge".to_string(), Seen { version: "1.2.0".into(), first_seen: 0, last_seen: 1000 });
        seen.insert("chrome".to_string(), Seen { version: "1.2.0".into(), first_seen: 0, last_seen: 1000 });
        let fresh = status_from(&[installed("edge"), installed("chrome")], &seen, 1000 + 60);
        assert!(fresh.iter().all(|b| b.connected && b.ever_connected));
        let stale = status_from(&[installed("edge"), installed("chrome")], &seen, 1000 + CONNECTED_WINDOW_SECS + 5);
        assert!(stale.iter().all(|b| !b.connected && b.ever_connected), "closed for a while: not connected, but it was set up once");
        let never = status_from(&[installed("firefox")], &HashMap::new(), 1000);
        assert!(!never[0].connected && !never[0].ever_connected && never[0].family == "firefox");
    }

    #[test]
    fn hello_from_nonsense_names_is_ignored() {
        let before = seen_store().lock().len();
        note_hello("../../etc", "1");
        note_hello("", "1");
        note_hello(&"a".repeat(40), "1");
        assert_eq!(seen_store().lock().len(), before);
    }

    #[test]
    fn chromium_browsers_open_their_own_extensions_page() {
        assert_eq!(chromium_scheme("edge"), "edge");
        assert_eq!(chromium_scheme("brave"), "brave");
        assert_eq!(chromium_scheme("chrome"), "chrome");
    }
}
