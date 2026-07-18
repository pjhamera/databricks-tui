//! Next-fire computation for job schedules: Quartz cron expressions,
//! periodic triggers, file-arrival triggers and continuous mode.

use chrono::TimeZone;
use chrono_tz::Tz;
use cron::Schedule;
use serde_json::Value;
use std::str::FromStr;

/// When a job will run next, derived from its settings.
pub struct NextRun {
    /// Epoch ms of the next execution; None when it can't be predicted
    /// (paused, file-arrival, unparseable cron).
    pub at_ms: Option<u64>,
    /// Short label for list rows, e.g. "in 27m", "paused", "on file arrival".
    pub label: String,
    /// Longer description for the upcoming view, e.g. the cron expression.
    pub desc: String,
    pub paused: bool,
}

/// Compact countdown to a future epoch-ms instant, e.g. "in 2h 15m".
pub fn fmt_eta(at_ms: u64, now_ms: u64) -> String {
    let secs = at_ms.saturating_sub(now_ms) / 1000;
    match secs {
        0..=59 => "in <1m".to_string(),
        60..=3599 => format!("in {}m", secs / 60),
        3600..=86_399 => format!("in {}h {}m", secs / 3600, (secs % 3600) / 60),
        _ => format!("in {}d", secs / 86_400),
    }
}

/// Next fire of a Quartz cron expression in an IANA timezone, as epoch ms.
/// Unknown timezones fall back to UTC; an unparseable cron yields None.
pub fn next_fire(cron: &str, tz: &str, now_ms: u64) -> Option<u64> {
    let schedule = Schedule::from_str(cron.trim()).ok()?;
    let tz: Tz = tz.parse().unwrap_or(chrono_tz::UTC);
    let now = tz.timestamp_millis_opt(now_ms as i64).single()?;
    schedule
        .after(&now)
        .next()
        .map(|dt| dt.timestamp_millis() as u64)
}

/// Derives the next run from a job's settings (`schedule`, `trigger`,
/// `continuous`), using the last run start for periodic triggers.
/// None for jobs that only run on demand.
pub fn next_run(settings: &Value, last_start_ms: Option<u64>, now_ms: u64) -> Option<NextRun> {
    if let Some(cron) = settings["schedule"]["quartz_cron_expression"].as_str() {
        let tz = settings["schedule"]["timezone_id"]
            .as_str()
            .unwrap_or("UTC");
        let paused = settings["schedule"]["pause_status"].as_str() == Some("PAUSED");
        let desc = format!("cron {cron} · {tz}");
        if paused {
            return Some(NextRun {
                at_ms: None,
                label: "⏸ paused".to_string(),
                desc,
                paused: true,
            });
        }
        return Some(match next_fire(cron, tz, now_ms) {
            Some(at) => NextRun {
                at_ms: Some(at),
                label: fmt_eta(at, now_ms),
                desc,
                paused: false,
            },
            None => NextRun {
                at_ms: None,
                label: "scheduled".to_string(),
                desc,
                paused: false,
            },
        });
    }

    let continuous = &settings["continuous"];
    if continuous.is_object() {
        let paused = continuous["pause_status"].as_str() == Some("PAUSED");
        return Some(NextRun {
            at_ms: None,
            label: if paused { "⏸ paused" } else { "continuous" }.to_string(),
            desc: "continuous".to_string(),
            paused,
        });
    }

    let trigger = &settings["trigger"];
    if trigger.is_object() {
        let paused = trigger["pause_status"].as_str() == Some("PAUSED");
        if let (Some(n), Some(unit)) = (
            trigger["periodic"]["interval"].as_u64(),
            trigger["periodic"]["unit"].as_str(),
        ) {
            let unit_ms = match unit {
                "HOURS" => 3_600_000,
                "DAYS" => 86_400_000,
                "WEEKS" => 604_800_000,
                _ => 0,
            };
            let unit_word = unit.to_lowercase();
            let unit_word = if n == 1 {
                unit_word.trim_end_matches('s')
            } else {
                &unit_word
            };
            let desc = format!("every {n} {unit_word}");
            if paused {
                return Some(NextRun {
                    at_ms: None,
                    label: "⏸ paused".to_string(),
                    desc,
                    paused: true,
                });
            }
            // The API doesn't expose the next fire directly; estimate it
            // from the last run's start.
            let at = last_start_ms
                .filter(|_| unit_ms > 0)
                .map(|s| s + n * unit_ms)
                .filter(|at| *at > now_ms);
            return Some(match at {
                Some(at) => NextRun {
                    at_ms: Some(at),
                    label: format!("~{}", fmt_eta(at, now_ms)),
                    desc,
                    paused: false,
                },
                None => NextRun {
                    at_ms: None,
                    label: desc.clone(),
                    desc,
                    paused: false,
                },
            });
        }
        if trigger["file_arrival"].is_object() {
            return Some(NextRun {
                at_ms: None,
                label: if paused {
                    "⏸ paused"
                } else {
                    "on file arrival"
                }
                .to_string(),
                desc: "file arrival trigger".to_string(),
                paused,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // 2026-07-18 12:00:00 UTC
    const NOW: u64 = 1_784_376_000_000;

    #[test]
    fn quartz_daily_cron_fires_next_day() {
        // Daily at 02:00 UTC — next fire is tomorrow 02:00.
        let at = next_fire("0 0 2 * * ?", "UTC", NOW).unwrap();
        assert!(at > NOW);
        assert_eq!((at - NOW) % 86_400_000 % 3_600_000, 0);
        assert!(at - NOW < 86_400_000);
    }

    #[test]
    fn timezone_shifts_the_fire_time() {
        let utc = next_fire("0 0 2 * * ?", "UTC", NOW).unwrap();
        let tokyo = next_fire("0 0 2 * * ?", "Asia/Tokyo", NOW).unwrap();
        assert_ne!(utc, tokyo);
    }

    #[test]
    fn bad_cron_is_none() {
        assert!(next_fire("not a cron", "UTC", NOW).is_none());
    }

    #[test]
    fn paused_schedule_has_no_time() {
        let s = json!({"schedule": {
            "quartz_cron_expression": "0 0 2 * * ?",
            "timezone_id": "UTC",
            "pause_status": "PAUSED"
        }});
        let n = next_run(&s, None, NOW).unwrap();
        assert!(n.paused);
        assert!(n.at_ms.is_none());
    }

    #[test]
    fn periodic_trigger_estimates_from_last_start() {
        let s = json!({"trigger": {"periodic": {"interval": 4, "unit": "HOURS"}}});
        let n = next_run(&s, Some(NOW - 3_600_000), NOW).unwrap();
        assert_eq!(n.at_ms, Some(NOW + 3 * 3_600_000));
        assert_eq!(n.label, "~in 3h 0m");
    }

    #[test]
    fn on_demand_job_is_none() {
        assert!(next_run(&json!({"name": "adhoc"}), None, NOW).is_none());
    }

    #[test]
    fn eta_formats() {
        assert_eq!(fmt_eta(NOW + 30_000, NOW), "in <1m");
        assert_eq!(fmt_eta(NOW + 27 * 60_000, NOW), "in 27m");
        assert_eq!(fmt_eta(NOW + 3 * 86_400_000, NOW), "in 3d");
    }
}
