//! "Find the download": for a link that is a web page with a download button instead of a
//! file. Nexdrop opens the page in its own browser view, presses the page's download button
//! (a small script, see `resolver.js`) and catches the file the page starts: the address, the
//! page it came from and the cookies the site gave. Only ever started by the user.
//!
//! If nothing starts within a few seconds (a countdown, a check only a person can pass) the
//! view is shown so the user can finish it; whatever download then starts is caught the same way.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Url, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::mpsc;

const SCRIPT: &str = include_str!("resolver.js");
/// The same identity for the page and for the download that follows, so signed links keep working.
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0";
/// How long the view stays hidden before it is shown for the user to finish.
const SHOW_AFTER: Duration = Duration::from_secs(12);
const GIVE_UP_AFTER: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedPage {
    /// The file's address.
    pub url: String,
    /// The page the download started from.
    pub referer: String,
    pub user_agent: String,
    /// `name=value; ...` for the file's site, if the page set any.
    pub cookie: Option<String>,
    /// The view had to be shown to the user.
    pub needed_user: bool,
}

enum Event {
    File(Url),
    Open(Url),
    Closed,
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);
static ACTIVE: Mutex<Option<mpsc::UnboundedSender<Event>>> = Mutex::new(None);

/// File addresses a page may send the view to directly (shown inline by the browser otherwise).
fn looks_like_file(url: &Url) -> bool {
    const EXT: &[&str] = &[
        "zip", "rar", "7z", "tar", "gz", "tgz", "xz", "bz2", "iso", "exe", "msi", "dmg", "apk", "pkg", "deb", "rpm", "mp4", "mkv", "webm", "avi", "mov",
        "m4v", "mp3", "flac", "wav", "m4a", "pdf", "epub", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "bin", "torrent",
    ];
    let last = url.path_segments().and_then(|mut s| s.next_back()).unwrap_or("");
    last.rsplit_once('.').map(|(_, e)| EXT.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

/// A scratch browser profile for the helper view, emptied before each use.
fn profile_dir() -> std::path::PathBuf {
    let dir = crate::util::data_dir().join("resolver-profile");
    let _ = std::fs::remove_dir_all(&dir); // best effort: what an earlier search left behind
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Stops a search in progress (the user closed the dialog).
pub fn cancel() {
    if let Some(tx) = ACTIVE.lock().take() {
        let _ = tx.send(Event::Closed);
    }
}

pub async fn resolve(app: &AppHandle, page_url: &str) -> Result<ResolvedPage, String> {
    let page = Url::parse(page_url.trim()).map_err(|e| format!("Invalid link: {e}"))?;
    if !matches!(page.scheme(), "http" | "https") {
        return Err("Only http and https pages can be opened".to_string());
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
    {
        let mut active = ACTIVE.lock();
        if active.is_some() {
            return Err("Already looking for a download; wait for it or cancel it first".to_string());
        }
        *active = Some(tx.clone());
    }
    let result = run(app, page, tx, &mut rx).await;
    ACTIVE.lock().take();
    result
}

async fn run(app: &AppHandle, page: Url, tx: mpsc::UnboundedSender<Event>, rx: &mut mpsc::UnboundedReceiver<Event>) -> Result<ResolvedPage, String> {
    let label = format!("resolver-{}", COUNTER.fetch_add(1, Ordering::Relaxed));
    let host = page.host_str().unwrap_or("the site").to_string();
    let current_page = Arc::new(Mutex::new(page.to_string()));

    let (t_dl, t_nav, t_win) = (tx.clone(), tx.clone(), tx.clone());
    let seen_page = current_page.clone();
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(page.clone()))
        .title(format!("Nexdrop · {host}"))
        .inner_size(560.0, 720.0)
        .visible(false)
        .data_directory(profile_dir()) // its own profile: nothing from the site mixes with Nexdrop's or stays
        .user_agent(USER_AGENT)
        .initialization_script(SCRIPT)
        // The page starts a download: take its address and don't let the view save it.
        .on_download(move |_, event| {
            if let DownloadEvent::Requested { url, .. } = event {
                let _ = t_dl.send(Event::File(url));
            }
            false
        })
        // A page sending the view straight to a file (a video would play inline): same.
        .on_navigation(move |url| {
            if looks_like_file(url) {
                let _ = t_nav.send(Event::File(url.clone()));
                return false;
            }
            matches!(url.scheme(), "http" | "https" | "about")
        })
        // Pop-ups stay out; where they were heading is opened in this same view instead.
        .on_new_window(move |url, _| {
            let _ = t_win.send(Event::Open(url));
            NewWindowResponse::Deny
        })
        .on_page_load(move |_, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                *seen_page.lock() = payload.url().to_string();
            }
        })
        .build()
        .map_err(|e| format!("Couldn't open the page: {e}"))?;

    let closer = tx.clone();
    window.on_window_event(move |e| {
        if matches!(e, tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed) {
            let _ = closer.send(Event::Closed);
        }
    });

    let started = Instant::now();
    let mut shown = false;
    let outcome: Result<Url, String> = loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Some(Event::File(url)) => break Ok(url),
                Some(Event::Open(url)) if matches!(url.scheme(), "http" | "https") => {
                    if looks_like_file(&url) {
                        break Ok(url);
                    }
                    let _ = window.navigate(url);
                }
                Some(Event::Open(_)) => {}
                Some(Event::Closed) | None => break Err("Cancelled".to_string()),
            },
            _ = tokio::time::sleep(Duration::from_millis(400)) => {
                let waited = started.elapsed();
                if !shown && waited >= SHOW_AFTER {
                    shown = true;
                    let _ = window.set_title(&format!("Nexdrop · {host} — press the download button"));
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                if waited >= GIVE_UP_AFTER {
                    break Err("The page didn't start a download. Open it in your browser instead.".to_string());
                }
            }
        }
    };

    let file = match outcome {
        Ok(url) => url,
        Err(e) => {
            let _ = window.destroy();
            return Err(e);
        }
    };
    if !matches!(file.scheme(), "http" | "https") {
        let _ = window.destroy();
        return Err("The page creates the file inside the browser, so Nexdrop can't take it over. Use your browser for this one.".to_string());
    }

    // The cookies the site set for the file's address (the signed link may need them).
    let cookie_window = window.clone();
    let cookie_url = file.clone();
    let cookie = tauri::async_runtime::spawn_blocking(move || {
        cookie_window.cookies_for_url(cookie_url).ok().map(|cookies| {
            cookies.iter().map(|c| format!("{}={}", c.name(), c.value())).collect::<Vec<_>>().join("; ")
        })
    })
    .await
    .ok()
    .flatten()
    .filter(|c| !c.is_empty());

    let referer = current_page.lock().clone();
    let _ = window.destroy();
    Ok(ResolvedPage { url: file.to_string(), referer, user_agent: USER_AGENT.to_string(), cookie, needed_user: shown })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_file_addresses_are_taken_directly() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(looks_like_file(&u("https://cdn.example.com/a/b/movie.MP4?token=1")));
        assert!(looks_like_file(&u("https://x.test/setup.exe")));
        assert!(!looks_like_file(&u("https://x.test/file/123")));
        assert!(!looks_like_file(&u("https://x.test/page.html")));
        assert!(!looks_like_file(&u("https://x.test/")));
    }
}
