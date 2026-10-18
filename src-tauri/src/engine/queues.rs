//! Download queues: when a queue may run, and when it next will.
//!
//! A queue either runs whenever it has work, or only inside its schedule:
//!
//! * **Start and stop time**: open from start to stop on the chosen days
//!   (a window may cross midnight; the selected days are the days it *opens* on).
//! * **Start time only**: at the start time the queue starts and keeps running
//!   until it is empty.
//!
//! "Start now" opens a queue regardless of its schedule until it empties (or
//! is stopped). Everything here is plain functions over `DownloadQueue`, so it
//! is exhaustively testable without the app running.

use chrono::{DateTime, Datelike, Duration, Local, NaiveTime, TimeZone, Timelike};

use crate::db::models::{AppSettings, DownloadQueue, MAIN_QUEUE_ID};
use crate::engine::scheduler::{parse_hhmm, window_open_at};

pub const AFTER_DONE_ACTIONS: [&str; 8] = ["none", "notify", "exit", "sleep", "hibernate", "shutdown", "lock", "program"];

/// The built-in queue, created from the old single-scheduler settings so
/// nobody loses their schedule when upgrading.
pub fn main_queue_from_settings(s: &AppSettings) -> DownloadQueue {
    let migrated_action = match s.scheduler_action.as_str() {
        "sleep" | "shutdown" | "exit" => s.scheduler_action.clone(),
        _ => "none".to_string(),
    };
    DownloadQueue {
        id: MAIN_QUEUE_ID.to_string(),
        name: "Main queue".to_string(),
        schedule_enabled: s.scheduler_enabled,
        days: if s.scheduler_days.is_empty() { DownloadQueue::default().days } else { s.scheduler_days.clone() },
        start_time: s.scheduler_start_time.clone().unwrap_or_else(|| "02:00".to_string()),
        stop_time: s.scheduler_stop_time.clone().filter(|t| parse_hhmm(t).is_some()),
        speed_limit: s.scheduler_speed_limit,
        after_done: if s.scheduler_enabled { migrated_action } else { "none".to_string() },
        position: 0,
        ..DownloadQueue::default()
    }
}

/// Cleans up what the interface sent so a bad value can never wedge a queue.
pub fn sanitize(mut q: DownloadQueue) -> Result<DownloadQueue, String> {
    q.name = q.name.trim().chars().take(40).collect();
    if q.name.is_empty() {
        return Err("Give the queue a name".to_string());
    }
    if parse_hhmm(&q.start_time).is_none() {
        return Err("The start time isn't a valid time".to_string());
    }
    q.stop_time = match q.stop_time.take().filter(|t| !t.trim().is_empty()) {
        Some(t) if parse_hhmm(&t).is_some() => Some(t),
        Some(_) => return Err("The stop time isn't a valid time".to_string()),
        None => None,
    };
    const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    q.days = DAYS.iter().filter(|d| q.days.iter().any(|x| x.eq_ignore_ascii_case(d))).map(|d| d.to_string()).collect();
    if q.schedule_enabled && q.days.is_empty() {
        return Err("Pick at least one day for the schedule".to_string());
    }
    if !AFTER_DONE_ACTIONS.contains(&q.after_done.as_str()) {
        q.after_done = "none".to_string();
    }
    q.after_done_program = q.after_done_program.take().map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    if q.after_done == "program" && q.after_done_program.is_none() {
        return Err("Choose the program to run when the queue finishes".to_string());
    }
    q.max_active = q.max_active.min(32);
    q.retry_count = q.retry_count.min(20);
    q.retry_delay_secs = q.retry_delay_secs.clamp(5, 3600);
    Ok(q)
}

/// May `q` start downloads at `now`?
pub fn is_open(q: &DownloadQueue, forced_open: bool, now: DateTime<Local>) -> bool {
    if !q.schedule_enabled || forced_open {
        return true;
    }
    match (parse_hhmm(&q.start_time), q.stop_time.as_deref()) {
        (Some(start), Some(stop)) => match parse_hhmm(stop) {
            Some(stop) => window_open_at(&q.days, start, stop, now.weekday(), now.hour() * 60 + now.minute()),
            None => true, // unreadable times never leave downloads stuck
        },
        (Some(_), None) => false, // start-only: opened by its start-time trigger
        (None, _) => true,
    }
}

/// Did a start-only schedule's start time fall in `(after, now]` on a selected day?
pub fn start_crossed(q: &DownloadQueue, after: DateTime<Local>, now: DateTime<Local>) -> bool {
    if !q.schedule_enabled || q.stop_time.is_some() {
        return false;
    }
    let Some(start) = parse_hhmm(&q.start_time) else { return false };
    let on_day = |d: chrono::NaiveDate| -> Option<DateTime<Local>> {
        let t = NaiveTime::from_hms_opt(start / 60, start % 60, 0)?;
        Local.from_local_datetime(&d.and_time(t)).single()
    };
    // The span can cover two calendar days (across midnight).
    [after.date_naive(), now.date_naive()].iter().any(|&d| {
        let selected = q.days.iter().any(|x| x.eq_ignore_ascii_case(day_key(d.weekday())));
        selected && on_day(d).map(|at| at > after && at <= now).unwrap_or(false)
    })
}

/// When does the schedule next open? `None` for queues without a schedule.
pub fn next_start(q: &DownloadQueue, now: DateTime<Local>) -> Option<DateTime<Local>> {
    if !q.schedule_enabled {
        return None;
    }
    let start = parse_hhmm(&q.start_time)?;
    let t = NaiveTime::from_hms_opt(start / 60, start % 60, 0)?;
    for offset in 0..8 {
        let d = now.date_naive() + Duration::days(offset);
        if !q.days.iter().any(|x| x.eq_ignore_ascii_case(day_key(d.weekday()))) {
            continue;
        }
        if let Some(at) = Local.from_local_datetime(&d.and_time(t)).single() {
            if at > now {
                return Some(at);
            }
        }
    }
    None
}

fn day_key(w: chrono::Weekday) -> &'static str {
    match w {
        chrono::Weekday::Mon => "mon",
        chrono::Weekday::Tue => "tue",
        chrono::Weekday::Wed => "wed",
        chrono::Weekday::Thu => "thu",
        chrono::Weekday::Fri => "fri",
        chrono::Weekday::Sat => "sat",
        chrono::Weekday::Sun => "sun",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, h, min, 0).single().unwrap()
    }

    // 2026-10-19 is a Monday.
    fn q() -> DownloadQueue {
        DownloadQueue { id: "q".into(), schedule_enabled: true, start_time: "02:00".into(), stop_time: Some("07:00".into()), ..Default::default() }
    }

    #[test]
    fn unscheduled_queues_are_always_open() {
        let mut queue = q();
        queue.schedule_enabled = false;
        assert!(is_open(&queue, false, at(2026, 10, 19, 12, 0)));
    }

    #[test]
    fn windowed_queue_follows_its_window() {
        let queue = q();
        assert!(!is_open(&queue, false, at(2026, 10, 19, 1, 59)));
        assert!(is_open(&queue, false, at(2026, 10, 19, 2, 0)));
        assert!(!is_open(&queue, false, at(2026, 10, 19, 7, 0)));
        assert!(is_open(&queue, true, at(2026, 10, 19, 12, 0)), "Start now overrides the schedule");
    }

    #[test]
    fn start_only_queue_is_closed_until_triggered() {
        let mut queue = q();
        queue.stop_time = None;
        assert!(!is_open(&queue, false, at(2026, 10, 19, 3, 0)));
        assert!(is_open(&queue, true, at(2026, 10, 19, 3, 0)));
    }

    #[test]
    fn start_time_trigger_fires_once_when_crossed() {
        let mut queue = q();
        queue.stop_time = None;
        let before = at(2026, 10, 19, 1, 59);
        let after = at(2026, 10, 19, 2, 1);
        assert!(start_crossed(&queue, before, after));
        assert!(!start_crossed(&queue, after, at(2026, 10, 19, 2, 5)), "already past");
        assert!(!start_crossed(&queue, at(2026, 10, 19, 3, 0), at(2026, 10, 19, 3, 1)));
    }

    #[test]
    fn start_trigger_respects_selected_days_and_midnight() {
        let mut queue = q();
        queue.stop_time = None;
        queue.start_time = "00:00".into();
        queue.days = vec!["tue".into()];
        // Crossing midnight into Tuesday fires; into Wednesday does not.
        assert!(start_crossed(&queue, at(2026, 10, 19, 23, 59), at(2026, 10, 20, 0, 1)));
        assert!(!start_crossed(&queue, at(2026, 10, 20, 23, 59), at(2026, 10, 21, 0, 1)));
    }

    #[test]
    fn windowed_queues_have_no_trigger() {
        assert!(!start_crossed(&q(), at(2026, 10, 19, 1, 0), at(2026, 10, 19, 3, 0)));
    }

    #[test]
    fn next_start_finds_the_following_selected_day() {
        let mut queue = q();
        queue.days = vec!["wed".into()];
        let next = next_start(&queue, at(2026, 10, 19, 12, 0)).unwrap();
        assert_eq!((next.month(), next.day(), next.hour()), (10, 21, 2));
        queue.days = vec!["mon".into()];
        let later = next_start(&queue, at(2026, 10, 19, 12, 0)).unwrap();
        assert_eq!(later.day(), 26, "today's start has passed, so next Monday");
        let today = next_start(&queue, at(2026, 10, 19, 1, 0)).unwrap();
        assert_eq!(today.day(), 19);
        queue.schedule_enabled = false;
        assert!(next_start(&queue, at(2026, 10, 19, 1, 0)).is_none());
    }

    #[test]
    fn sanitize_rejects_nonsense_and_normalises() {
        let mut bad = q();
        bad.name = "  ".into();
        assert!(sanitize(bad).is_err());

        let mut bad = q();
        bad.start_time = "25:00".into();
        assert!(sanitize(bad).is_err());

        let mut bad = q();
        bad.days.clear();
        assert!(sanitize(bad).is_err());

        let mut bad = q();
        bad.after_done = "program".into();
        assert!(sanitize(bad).is_err(), "a program action needs a program");

        let mut ok = q();
        ok.name = "  Night downloads  ".into();
        ok.days = vec!["SUN".into(), "mon".into(), "junk".into()];
        ok.after_done = "explode".into();
        ok.retry_delay_secs = 1;
        let ok = sanitize(ok).unwrap();
        assert_eq!(ok.name, "Night downloads");
        assert_eq!(ok.days, vec!["mon", "sun"]);
        assert_eq!(ok.after_done, "none");
        assert_eq!(ok.retry_delay_secs, 5);
    }

    #[test]
    fn old_scheduler_settings_become_the_main_queue() {
        let mut s = AppSettings::default();
        s.scheduler_enabled = true;
        s.scheduler_start_time = Some("23:00".into());
        s.scheduler_stop_time = Some("06:00".into());
        s.scheduler_speed_limit = 500_000;
        s.scheduler_action = "shutdown".into();
        let main = main_queue_from_settings(&s);
        assert_eq!(main.id, MAIN_QUEUE_ID);
        assert!(main.schedule_enabled);
        assert_eq!((main.start_time.as_str(), main.stop_time.as_deref()), ("23:00", Some("06:00")));
        assert_eq!((main.speed_limit, main.after_done.as_str()), (500_000, "shutdown"));

        let off = main_queue_from_settings(&AppSettings::default());
        assert!(!off.schedule_enabled);
        assert_eq!(off.after_done, "none");
    }
}
