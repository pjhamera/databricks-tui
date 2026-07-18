use crate::cli::DatabricksCli;
use crate::schedule::{self, NextRun};

/// One scheduled/triggered job in the upcoming view.
pub struct UpcomingJob {
    pub name: String,
    pub next: NextRun,
}

/// All jobs that will run again on their own (cron schedule, periodic or
/// file-arrival trigger, continuous), soonest first; jobs without a
/// predictable time (paused, file-arrival) sort last. On-demand-only
/// jobs are omitted.
pub async fn fetch(cli: &DatabricksCli) -> Result<Vec<UpcomingJob>, String> {
    let json = cli
        .run(&["jobs", "list"])
        .await
        .map_err(|e| format!("{e:#}"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut items: Vec<UpcomingJob> = json
        .as_array()
        .map(|jobs| {
            jobs.iter()
                .filter_map(|j| {
                    // Periodic estimates need the last run start, which the
                    // job list doesn't carry — label falls back to the cadence.
                    let next = schedule::next_run(&j["settings"], None, now)?;
                    Some(UpcomingJob {
                        name: j["settings"]["name"]
                            .as_str()
                            .unwrap_or("unknown")
                            .to_string(),
                        next,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort_by_key(|u| (u.next.at_ms.unwrap_or(u64::MAX), u.name.clone()));
    Ok(items)
}
