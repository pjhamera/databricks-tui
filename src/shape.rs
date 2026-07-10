use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Status {
    Running,
    Success,
    Stopped,
    Pending,
    Failed,
    Unknown(String),
}

impl FromStr for Status {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_uppercase().as_str() {
            "RUNNING" => Status::Running,
            "SUCCESS" | "COMPLETED" | "FINISHED" => Status::Success,
            "IDLE" | "STOPPED" | "TERMINATED" | "DELETED" | "SKIPPED" | "CANCELED" => {
                Status::Stopped
            }
            "PENDING" | "STARTING" | "RESTARTING" | "DELETING" | "TERMINATING" | "QUEUED"
            | "WAITING_FOR_RETRY" | "BLOCKED" => Status::Pending,
            "FAILED" | "ERROR" | "TIMEDOUT" | "TIMED_OUT" | "INTERNAL_ERROR" => Status::Failed,
            other => Status::Unknown(other.to_string()),
        })
    }
}

impl Status {
    pub fn label(&self) -> &str {
        match self {
            Status::Running => "RUNNING",
            Status::Success => "SUCCESS",
            Status::Stopped => "IDLE",
            Status::Pending => "PENDING",
            Status::Failed => "FAILED",
            Status::Unknown(s) => s.as_str(),
        }
    }
}

/// Human-friendly "how long ago" for a millisecond epoch timestamp.
pub fn relative_time(epoch_ms: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let secs = now.saturating_sub(epoch_ms) / 1000;
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

/// Compact duration for a millisecond span, e.g. "12m 30s".
pub fn fmt_duration_ms(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[derive(Debug, Clone)]
pub enum Shape {
    List(Vec<ListItem>),
    Table(TableData),
    Badge(BadgeData),
    Text(String),
}

#[derive(Debug, Clone, Default)]
pub struct ListItem {
    pub name: String,
    pub status: Status,
    pub detail: Option<String>,
    /// Resource id used to fetch the full detail view.
    pub id: Option<String>,
    /// Recent run results, oldest first — rendered as a ✓/✗ strip.
    pub history: Vec<Status>,
}

impl Default for Status {
    fn default() -> Self {
        Status::Unknown(String::new())
    }
}

/// Structured content for the item detail view.
#[derive(Debug, Clone)]
pub struct DetailData {
    /// Key/value facts shown at the top.
    pub summary: Vec<(String, String)>,
    /// Recent events or runs, each with a status for the colored dot.
    pub activity: Vec<(Status, String)>,
    /// Full pretty-printed JSON, shown via the raw toggle.
    pub raw: String,
}

#[derive(Debug, Clone)]
pub struct TableData {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct BadgeData {
    pub label: String,
    pub value: String,
}
