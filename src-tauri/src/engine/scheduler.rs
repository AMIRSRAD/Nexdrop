//! Time-based scheduling.
//!
//! Two independent jobs share one background loop:
//!
//! 1. **Per-download schedules**: a download added with "Schedule" sits in the
//!    `Scheduled` state and is queued once its time has come (also after the
//!    app was closed or the PC asleep at that moment).
//! 2. **The scheduler window** (Settings → Scheduler): downloads run during a
//!    daily time window on chosen days. When the window closes, running
//!    downloads are paused and remembered; when it opens again they resume.
//!    A speed cap can apply inside the window, and a "when the window closes"
//!    action (sleep / shut down / quit) can follow.
//!
//! The window is evaluated as "are we inside it right now?", never as "is it
//! exactly the start minute?", so a sleeping PC or a late launch can't miss it.

use std::sync::Arc;
use std::time::Duration;

use chrono::{Datelike, Local, Timelike, Weekday};
use tauri::{AppHandle, Emitter};

use crate::db::models::AppSettings;
use crate::engine::DownloadManager;

const TICK: Duration = Duration::from_secs(5);

/// "HH:MM" → minutes since midnight.
pub fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

fn weekday_key(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "mon",
        Weekday::Tue => "tue",
        Weekday::Wed => "wed",
        Weekday::Thu => "thu",
        Weekday::Fri => "fri",
        Weekday::Sat => "sat",
        Weekday::Sun => "sun",
    }
}

/// Is the moment (`weekday`, `minute_of_day`) inside the window that opens at
/// `start` and closes at `stop` (both minutes since midnight)?
///
/// A window may cross midnight (e.g. 23:00 → 06:00). The selected days are the
/// days the window *opens* on, so at 02:00 on Tuesday that overnight window
/// counts as open only if Monday is selected.
pub fn window_open_at(days: &[String], start: u32, stop: u32, weekday: Weekday, minute_of_day: u32) -> bool {
    let selected = |w: Weekday| days.iter().any(|d| d.trim().eq_ignore_ascii_case(weekday_key(w)));
    if start == stop {
        // No gap between start and stop: open all day on the selected days.
        return selected(weekday);
    }
    if start < stop {
        selected(weekday) && minute_of_day >= start && minute_of_day < stop
    } else {
        (minute_of_day >= start && selected(weekday)) || (minute_of_day < stop && selected(weekday.pred()))
    }
}

/// Is the scheduler window open right now? (Treats unreadable times as "always
/// open" so a typo in settings can never leave downloads stuck forever.)
pub fn window_is_open_now(settings: &AppSettings) -> bool {
    let start = settings.scheduler_start_time.as_deref().and_then(parse_hhmm);
    let stop = settings.scheduler_stop_time.as_deref().and_then(parse_hhmm);
    let (Some(start), Some(stop)) = (start, stop) else { return true };
    let now = Local::now();
    window_open_at(&settings.scheduler_days, start, stop, now.weekday(), now.hour() * 60 + now.minute())
}

/// Scheduled times are stored in seconds, but older/browser-mode data used milliseconds.
pub fn to_unix_seconds(t: i64) -> i64 {
    if t > 100_000_000_000 {
        t / 1000
    } else {
        t
    }
}

pub struct DownloadScheduler;

impl DownloadScheduler {
    pub fn start_loop(app_handle: AppHandle, manager: Arc<DownloadManager>) {
        tauri::async_runtime::spawn(async move {
            // None until the first evaluation, so we never "close" a window we never saw open.
            let mut was_open: Option<bool> = None;

            loop {
                tokio::time::sleep(TICK).await;

                manager.start_due_scheduled().await;

                let settings = manager.get_settings();
                if !settings.scheduler_enabled {
                    if was_open.take().is_some() {
                        // Switched off: lift the speed cap and let paused downloads continue.
                        manager.set_scheduler_window(false);
                        manager.scheduler_resume().await;
                    }
                    continue;
                }

                let open = window_is_open_now(&settings);
                match (was_open, open) {
                    (Some(prev), now) if prev == now => {}
                    (None, true) => {
                        manager.set_scheduler_window(true);
                        manager.scheduler_resume().await;
                    }
                    (None, false) => manager.set_scheduler_window(false),
                    (Some(false), true) => {
                        manager.set_scheduler_window(true);
                        let _ = app_handle.emit("scheduler-event", serde_json::json!({ "event": "started" }));
                        manager.scheduler_resume().await;
                    }
                    (Some(true), false) => {
                        manager.set_scheduler_window(false);
                        let paused = manager.scheduler_pause_active().await;
                        let _ = app_handle.emit(
                            "scheduler-event",
                            serde_json::json!({ "event": "stopped", "paused": paused, "action": settings.scheduler_action }),
                        );
                        // The action follows only if the window actually cut something short.
                        if paused > 0 {
                            Self::run_close_action(&app_handle, &manager, &settings.scheduler_action);
                        }
                    }
                    _ => {}
                }
                was_open = Some(open);
            }
        });
    }

    fn run_close_action(app: &AppHandle, manager: &Arc<DownloadManager>, action: &str) {
        match action {
            "sleep" => manager.schedule_power_action("sleep", 30),
            "shutdown" => manager.schedule_power_action("shutdown", 60),
            "exit" => app.exit(0),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_days() -> Vec<String> {
        ["mon", "tue", "wed", "thu", "fri", "sat", "sun"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_times() {
        assert_eq!(parse_hhmm("02:00"), Some(120));
        assert_eq!(parse_hhmm("23:59"), Some(1439));
        assert_eq!(parse_hhmm(" 7:05 "), Some(425));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("nope"), None);
    }

    #[test]
    fn daytime_window() {
        let days = all_days();
        let (start, stop) = (2 * 60, 7 * 60);
        assert!(!window_open_at(&days, start, stop, Weekday::Mon, 119));
        assert!(window_open_at(&days, start, stop, Weekday::Mon, 120), "opens exactly at start");
        assert!(window_open_at(&days, start, stop, Weekday::Mon, 6 * 60 + 59));
        assert!(!window_open_at(&days, start, stop, Weekday::Mon, 7 * 60), "closed exactly at stop");
    }

    #[test]
    fn only_selected_days_count() {
        let days = vec!["sat".to_string(), "sun".to_string()];
        assert!(window_open_at(&days, 60, 120, Weekday::Sat, 90));
        assert!(!window_open_at(&days, 60, 120, Weekday::Wed, 90));
    }

    #[test]
    fn overnight_window_spans_midnight_and_follows_the_opening_day() {
        // Opens 23:00, closes 06:00, selected: Friday only.
        let days = vec!["fri".to_string()];
        let (start, stop) = (23 * 60, 6 * 60);
        assert!(window_open_at(&days, start, stop, Weekday::Fri, 23 * 60 + 30), "Friday night");
        assert!(window_open_at(&days, start, stop, Weekday::Sat, 2 * 60), "early Saturday belongs to Friday's window");
        assert!(!window_open_at(&days, start, stop, Weekday::Fri, 2 * 60), "early Friday belongs to Thursday's");
        assert!(!window_open_at(&days, start, stop, Weekday::Sat, 23 * 60 + 30), "Saturday night isn't selected");
        assert!(!window_open_at(&days, start, stop, Weekday::Sat, 6 * 60), "closed at stop");
    }

    #[test]
    fn equal_start_and_stop_means_all_day() {
        let days = vec!["mon".to_string()];
        assert!(window_open_at(&days, 480, 480, Weekday::Mon, 0));
        assert!(window_open_at(&days, 480, 480, Weekday::Mon, 1439));
        assert!(!window_open_at(&days, 480, 480, Weekday::Tue, 600));
    }

    #[test]
    fn unreadable_times_never_block_downloads() {
        let mut s = AppSettings::default();
        s.scheduler_start_time = Some("soon".into());
        s.scheduler_stop_time = Some("later".into());
        assert!(window_is_open_now(&s));
    }

    #[test]
    fn schedule_times_accept_seconds_and_milliseconds() {
        assert_eq!(to_unix_seconds(1_700_000_000), 1_700_000_000);
        assert_eq!(to_unix_seconds(1_700_000_000_000), 1_700_000_000);
    }
}
