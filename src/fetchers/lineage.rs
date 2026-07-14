use crate::cli::DatabricksCli;
use crate::fetchers::preview::run_sql;
use crate::shape::{DetailData, Status};
use std::collections::{HashMap, HashSet};

/// How many hops to walk in each direction.
const MAX_DEPTH: usize = 3;
/// Frontier cap per hop — keeps the IN() lists and query count sane.
const MAX_FRONTIER: usize = 15;

/// One hop of lineage edges touching `frontier`, as (source, target)
/// pairs from the last 90 days of events.
async fn hop(
    cli: &DatabricksCli,
    warehouse_id: &str,
    frontier: &[String],
    upstream: bool,
) -> Result<Vec<(String, String)>, String> {
    let list = frontier
        .iter()
        .take(MAX_FRONTIER)
        .map(|n| format!("'{}'", n.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    let cond = if upstream {
        format!("target_table_full_name IN ({list}) AND source_table_full_name IS NOT NULL")
    } else {
        format!("source_table_full_name IN ({list}) AND target_table_full_name IS NOT NULL")
    };
    let sql = format!(
        "SELECT DISTINCT source_table_full_name, target_table_full_name \
         FROM system.access.table_lineage \
         WHERE {cond} \
           AND event_time >= date_sub(current_date(), 90) \
         LIMIT 100"
    );
    let table = run_sql(cli, &sql, warehouse_id).await?;
    Ok(table
        .rows
        .iter()
        .filter_map(|r| match r.as_slice() {
            [s, t] if s != "␀" && t != "␀" => Some((s.clone(), t.clone())),
            _ => None,
        })
        .collect())
}

/// Walks up to MAX_DEPTH hops from `root`, returning adjacency:
/// node → neighbors one hop further away from the root.
async fn walk(
    cli: &DatabricksCli,
    warehouse_id: &str,
    root: &str,
    upstream: bool,
) -> Result<HashMap<String, Vec<String>>, String> {
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
    let mut visited: HashSet<String> = HashSet::from([root.to_string()]);
    let mut frontier: Vec<String> = vec![root.to_string()];
    for _ in 0..MAX_DEPTH {
        if frontier.is_empty() {
            break;
        }
        let edges = hop(cli, warehouse_id, &frontier, upstream).await?;
        let mut next: Vec<String> = Vec::new();
        for (source, target) in edges {
            // Away-from-root direction: parents when walking upstream,
            // children when walking downstream.
            let (near, far) = if upstream {
                (target, source)
            } else {
                (source, target)
            };
            if !frontier.contains(&near) {
                continue; // defensive: only expand the current frontier
            }
            let entry = adjacency.entry(near).or_default();
            if !entry.contains(&far) {
                entry.push(far.clone());
            }
            if visited.insert(far.clone()) {
                next.push(far);
            }
        }
        for children in adjacency.values_mut() {
            children.sort();
        }
        frontier = next;
    }
    Ok(adjacency)
}

/// Renders one direction as an indented tree with box-drawing guides.
fn render_tree(
    adjacency: &HashMap<String, Vec<String>>,
    node: &str,
    prefix: &str,
    status: Status,
    seen: &mut HashSet<String>,
    out: &mut Vec<(Status, String)>,
) {
    let Some(children) = adjacency.get(node) else {
        return;
    };
    let n = children.len();
    for (i, child) in children.iter().enumerate() {
        let last = i + 1 == n;
        let branch = if last { "└─ " } else { "├─ " };
        let cycle = !seen.insert(child.clone());
        let label = if cycle {
            format!("{prefix}{branch}{child} ↩")
        } else {
            format!("{prefix}{branch}{child}")
        };
        out.push((status.clone(), label));
        if !cycle {
            let next_prefix = format!("{prefix}{}", if last { "   " } else { "│  " });
            render_tree(adjacency, child, &next_prefix, status.clone(), seen, out);
        }
    }
}

fn count_nodes(adjacency: &HashMap<String, Vec<String>>) -> usize {
    let mut all: HashSet<&String> = HashSet::new();
    for children in adjacency.values() {
        all.extend(children.iter());
    }
    all.len()
}

/// Upstream and downstream lineage of one table as a tree, up to
/// MAX_DEPTH hops each way, from system.access.table_lineage.
pub async fn fetch(cli: &DatabricksCli, full_name: &str, warehouse_id: &str) -> DetailData {
    let up = walk(cli, warehouse_id, full_name, true).await;
    let (up, down) = match up {
        Ok(up) => match walk(cli, warehouse_id, full_name, false).await {
            Ok(down) => (up, down),
            Err(e) => return lineage_error(e),
        },
        Err(e) => return lineage_error(e),
    };

    let summary = vec![
        ("Object".to_string(), full_name.to_string()),
        ("Upstream".to_string(), count_nodes(&up).to_string()),
        ("Downstream".to_string(), count_nodes(&down).to_string()),
        (
            "Window".to_string(),
            format!("last 90 days · up to {MAX_DEPTH} hops"),
        ),
    ];

    let mut activity: Vec<(Status, String)> = Vec::new();
    if !up.is_empty() {
        activity.push((Status::Success, "▲ upstream".to_string()));
        let mut seen = HashSet::from([full_name.to_string()]);
        render_tree(
            &up,
            full_name,
            "",
            Status::Success,
            &mut seen,
            &mut activity,
        );
    }
    if !down.is_empty() {
        if !activity.is_empty() {
            activity.push((Status::Unknown(String::new()), String::new()));
        }
        activity.push((Status::Pending, "▼ downstream".to_string()));
        let mut seen = HashSet::from([full_name.to_string()]);
        render_tree(
            &down,
            full_name,
            "",
            Status::Pending,
            &mut seen,
            &mut activity,
        );
    }
    if activity.is_empty() {
        activity.push((
            Status::Unknown(String::new()),
            "no lineage events recorded in the window".to_string(),
        ));
    }

    let raw = activity
        .iter()
        .map(|(_, l)| l.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    DetailData {
        summary,
        activity,
        raw,
    }
}

fn lineage_error(e: String) -> DetailData {
    DetailData {
        summary: Vec::new(),
        activity: Vec::new(),
        raw: format!("{e}\n\nlineage needs read access to system.access.table_lineage"),
    }
}
