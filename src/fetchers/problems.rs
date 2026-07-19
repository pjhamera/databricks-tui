//! Cross-workspace problem scan: fetches the core panes of every other
//! configured profile and keeps what's failing, so `!` can show trouble
//! anywhere, not just in the current workspace.

use crate::cli::DatabricksCli;
use crate::shape::{Shape, Status};

/// One unhealthy resource found in another workspace.
pub struct RemoteProblem {
    pub profile: String,
    /// Index into `Panel::ALL`; None when the workspace itself is the
    /// problem (unreachable / auth expired).
    pub panel: Option<usize>,
    pub name: String,
    pub status: Status,
    pub note: String,
}

/// Scans every profile except `current`: clusters, jobs, pipelines and
/// warehouses per workspace, all in parallel. A workspace where every
/// fetch fails yields a single "unreachable" row instead of vanishing.
pub async fn fetch(profiles: Vec<String>, current: String) -> Vec<RemoteProblem> {
    let handles: Vec<_> = profiles
        .into_iter()
        .filter(|name| *name != current)
        .map(|name| tokio::spawn(scan_one(name)))
        .collect();
    let mut out = Vec::new();
    for h in handles {
        if let Ok(mut v) = h.await {
            out.append(&mut v);
        }
    }
    out
}

async fn scan_one(profile: String) -> Vec<RemoteProblem> {
    let arg = if profile == "DEFAULT" {
        None
    } else {
        Some(profile.clone())
    };
    let cli = DatabricksCli::new(arg);
    let (clusters, jobs, pipelines, warehouses) = tokio::join!(
        crate::fetchers::clusters::fetch(&cli),
        crate::fetchers::jobs::fetch(&cli),
        crate::fetchers::pipelines::fetch(&cli),
        crate::fetchers::warehouses::fetch(&cli),
    );

    let mut out = Vec::new();
    let mut reached = false;
    let mut first_err: Option<String> = None;
    // Panel indices match Panel::ALL: clusters 0, jobs 1, pipelines 2,
    // warehouses 3.
    for (panel, fetched) in [(0, clusters), (1, jobs), (2, pipelines), (3, warehouses)] {
        match fetched {
            Ok(Shape::List(items)) => {
                reached = true;
                for it in items {
                    let failed_now = matches!(it.status, Status::Failed);
                    let failed_last = it
                        .history
                        .last()
                        .is_some_and(|s| matches!(s, Status::Failed));
                    if failed_now || failed_last {
                        out.push(RemoteProblem {
                            profile: profile.clone(),
                            panel: Some(panel),
                            name: it.name,
                            status: it.status,
                            note: if failed_now {
                                it.detail.unwrap_or_default()
                            } else {
                                "latest run failed".to_string()
                            },
                        });
                    }
                }
            }
            Ok(_) => reached = true,
            Err(e) => {
                let msg = format!("{e:#}");
                first_err
                    .get_or_insert_with(|| msg.lines().next().unwrap_or("unreachable").to_string());
            }
        }
    }
    if !reached {
        out.push(RemoteProblem {
            profile: profile.clone(),
            panel: None,
            name: profile,
            status: Status::Failed,
            note: first_err.unwrap_or_else(|| "workspace unreachable".to_string()),
        });
    }
    out
}
