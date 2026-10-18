use std::sync::Arc;
use std::time::Duration;
use chrono::{Datelike, Local, Timelike};
use tauri::{AppHandle, Emitter};
use crate::db::models::AppSettings;
use crate::engine::DownloadManager;

pub struct DownloadScheduler;

impl DownloadScheduler {
    pub fn start_loop(app_handle: AppHandle, manager: Arc<DownloadManager>) {
        tauri::async_runtime::spawn(async move {
            let mut last_minute = 999;
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;

                let settings = manager.get_settings();
                if !settings.scheduler_enabled {
                    continue;
                }

                let now = Local::now();
                let current_minute = now.minute();
                let current_hour = now.hour();

                // Only evaluate once per minute
                if current_minute == last_minute {
                    continue;
                }
                last_minute = current_minute;

                let current_day = match now.weekday() {
                    chrono::Weekday::Mon => "mon",
                    chrono::Weekday::Tue => "tue",
                    chrono::Weekday::Wed => "wed",
                    chrono::Weekday::Thu => "thu",
                    chrono::Weekday::Fri => "fri",
                    chrono::Weekday::Sat => "sat",
                    chrono::Weekday::Sun => "sun",
                };

                let day_matches = settings
                    .scheduler_days
                    .iter()
                    .any(|d| d.to_lowercase() == current_day);

                if !day_matches {
                    continue;
                }

                let time_str = format!("{:02}:{:02}", current_hour, current_minute);

                // Check Start Time
                if let Some(ref start_time) = settings.scheduler_start_time {
                    if start_time == &time_str {
                        let _ = app_handle.emit(
                            "scheduler-event",
                            serde_json::json!({
                                "event": "started",
                                "time": time_str
                            }),
                        );
                        manager.process_queue().await;
                    }
                }

                // Check Stop Time
                if let Some(ref stop_time) = settings.scheduler_stop_time {
                    if stop_time == &time_str {
                        let _ = app_handle.emit(
                            "scheduler-event",
                            serde_json::json!({
                                "event": "stopped",
                                "time": time_str,
                                "action": settings.scheduler_action
                            }),
                        );

                        // Pause active downloads
                        let active = manager.get_active_task_ids();
                        for id in active {
                            let _ = manager.pause_download(&id);
                        }

                        // Execute post-schedule action
                        Self::execute_action(&settings);
                    }
                }
            }
        });
    }

    pub fn execute_action(settings: &AppSettings) {
        match settings.scheduler_action.as_str() {
            "sleep" => {
                #[cfg(target_os = "windows")]
                {
                    let _ = crate::process::command("rundll32.exe")
                        .args(["powrprof.dll,SetSuspendState", "0,1,0"])
                        .spawn();
                }
            }
            "shutdown" => {
                #[cfg(target_os = "windows")]
                {
                    let _ = crate::process::command("shutdown")
                        .args(["/s", "/t", "60"]) // 60 seconds grace period
                        .spawn();
                }
            }
            "exit" => {
                std::process::exit(0);
            }
            _ => {}
        }
    }
}
