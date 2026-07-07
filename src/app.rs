use crate::cli::DatabricksCli;
use crate::fetchers;
use crate::shape::{DetailData, Shape, Status};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThemeMode {
    Dark,
    Light,
}

impl ThemeMode {
    pub fn toggled(self) -> Self {
        match self {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Panel {
    Clusters,
    Jobs,
    Pipelines,
    Warehouses,
}

impl Panel {
    pub const ALL: &'static [Panel] = &[
        Panel::Clusters,
        Panel::Jobs,
        Panel::Pipelines,
        Panel::Warehouses,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Panel::Clusters => "Clusters",
            Panel::Jobs => "Jobs",
            Panel::Pipelines => "Pipelines",
            Panel::Warehouses => "Warehouses",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Panel::Clusters => "◆",
            Panel::Jobs => "◈",
            Panel::Pipelines => "⇶",
            Panel::Warehouses => "▣",
        }
    }

    /// The databricks CLI command group whose `get <id>` returns item details.
    pub fn cli_group(&self) -> &'static str {
        match self {
            Panel::Clusters => "clusters",
            Panel::Jobs => "jobs",
            Panel::Pipelines => "pipelines",
            Panel::Warehouses => "warehouses",
        }
    }
}

pub struct Detail {
    pub panel: Panel,
    pub name: String,
    pub id: String,
    /// None while the fetch is in flight.
    pub data: Option<DetailData>,
    /// Toggles between the formatted summary and the raw JSON.
    pub show_raw: bool,
    pub scroll: u16,
}

/// A pending destructive/mutating action awaiting a y/n keystroke.
pub struct Confirm {
    pub message: String,
    args: Vec<String>,
}

enum Update {
    Panel(usize, Result<Shape, String>),
    Badge(Option<Shape>),
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct App {
    pub focus: Panel,
    pub theme: ThemeMode,
    pub zoomed: bool,
    pub shapes: Vec<Option<Shape>>,
    pub user_badge: Option<Shape>,
    pub error: Option<String>,
    pub refresh_interval: Duration,
    last_refresh: Instant,
    pub loading: bool,
    pub detail: Option<Detail>,
    pub confirm: Option<Confirm>,
    pub flash: Option<(String, Instant)>,
    pub selected: [usize; 4],
    pub host: Option<String>,
    pending: Option<mpsc::UnboundedReceiver<Update>>,
    detail_rx: Option<oneshot::Receiver<DetailData>>,
    action_rx: Option<oneshot::Receiver<Result<String, String>>>,
    host_rx: Option<oneshot::Receiver<Option<String>>>,
    in_flight: usize,
    spinner_frame: usize,
}

impl App {
    pub fn new(refresh_secs: u64, theme: ThemeMode) -> Self {
        Self {
            focus: Panel::Clusters,
            theme,
            zoomed: false,
            shapes: vec![None, None, None, None],
            user_badge: None,
            error: None,
            refresh_interval: Duration::from_secs(refresh_secs),
            last_refresh: Instant::now()
                .checked_sub(Duration::from_secs(refresh_secs + 1))
                .unwrap_or(Instant::now()),
            loading: false,
            detail: None,
            confirm: None,
            flash: None,
            selected: [0; 4],
            host: None,
            pending: None,
            detail_rx: None,
            action_rx: None,
            host_rx: None,
            in_flight: 0,
            spinner_frame: 0,
        }
    }

    /// Resolves the workspace host in the background — `auth describe` can
    /// take seconds when it refreshes tokens, so it must not block the loop.
    pub fn fetch_host(&mut self, cli: &Arc<DatabricksCli>) {
        let (tx, rx) = oneshot::channel();
        self.host_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let host = cli.run(&["auth", "describe"]).await.ok().and_then(|json| {
                json["details"]["host"]
                    .as_str()
                    .or_else(|| json["host"].as_str())
                    .map(str::to_string)
            });
            let _ = tx.send(host);
        });
    }

    pub fn poll_host(&mut self) {
        if let Some(rx) = &mut self.host_rx {
            match rx.try_recv() {
                Ok(host) => {
                    self.host = host;
                    self.host_rx = None;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.host_rx = None;
                }
            }
        }
    }

    fn focus_index(&self) -> usize {
        Panel::ALL
            .iter()
            .position(|p| p == &self.focus)
            .unwrap_or(0)
    }

    fn list_len(&self, idx: usize) -> usize {
        match &self.shapes[idx] {
            Some(Shape::List(items)) => items.len(),
            _ => 0,
        }
    }

    /// Selection index for a panel, clamped to the current list length.
    pub fn selection(&self, idx: usize) -> usize {
        self.selected[idx].min(self.list_len(idx).saturating_sub(1))
    }

    pub fn select_next(&mut self) {
        let idx = self.focus_index();
        let len = self.list_len(idx);
        if len > 0 {
            self.selected[idx] = (self.selection(idx) + 1).min(len - 1);
        }
    }

    pub fn select_prev(&mut self) {
        let idx = self.focus_index();
        self.selected[idx] = self.selection(idx).saturating_sub(1);
    }

    /// The currently highlighted item in the focused panel.
    fn selected_item(&self) -> Option<&crate::shape::ListItem> {
        let idx = self.focus_index();
        match &self.shapes[idx] {
            Some(Shape::List(items)) => items.get(self.selection(idx)),
            _ => None,
        }
    }

    pub fn open_detail(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let Some(id) = item.id.clone() else {
            return;
        };
        self.detail = Some(Detail {
            panel: self.focus,
            name: item.name.clone(),
            id: id.clone(),
            data: None,
            show_raw: false,
            scroll: 0,
        });

        let (tx, rx) = oneshot::channel();
        self.detail_rx = Some(rx);
        let cli = Arc::clone(cli);
        let group = self.focus.cli_group();
        tokio::spawn(async move {
            let data = fetchers::detail::fetch(&cli, group, &id).await;
            let _ = tx.send(data);
        });
    }

    pub fn close_detail(&mut self) {
        self.detail = None;
        self.detail_rx = None;
    }

    pub fn toggle_raw(&mut self) {
        if let Some(d) = &mut self.detail {
            d.show_raw = !d.show_raw;
            d.scroll = 0;
        }
    }

    /// Applies a finished detail fetch; returns true if the UI should redraw.
    pub fn poll_detail(&mut self) -> bool {
        let Some(rx) = &mut self.detail_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(data) => {
                if let Some(d) = &mut self.detail {
                    d.data = Some(data);
                }
                self.detail_rx = None;
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.detail_rx = None;
                true
            }
        }
    }

    pub fn detail_scroll(&mut self, delta: i32) {
        if let Some(d) = &mut self.detail {
            let max = match &d.data {
                Some(data) if d.show_raw => data.raw.lines().count(),
                Some(data) => data.summary.len() + data.activity.len() + 3,
                None => 0,
            } as u16;
            d.scroll = if delta < 0 {
                d.scroll.saturating_sub(delta.unsigned_abs() as u16)
            } else {
                (d.scroll + delta as u16).min(max.saturating_sub(1))
            };
        }
    }

    /// Prepares a contextual action for the selected item, pending confirmation:
    /// start/stop for clusters, warehouses and pipelines, run-now for jobs.
    pub fn request_action(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let Some(id) = item.id.clone() else {
            return;
        };
        let name = item.name.clone();
        let active = matches!(
            item.status,
            Status::Running | Status::Pending | Status::Success
        );
        let group = self.focus.cli_group();
        let (verb, action): (&str, &str) = match self.focus {
            Panel::Jobs => ("Run", "run-now"),
            Panel::Clusters if active => ("Stop", "delete"),
            Panel::Pipelines if active => ("Stop", "stop"),
            Panel::Pipelines => ("Start update for", "start-update"),
            _ if active => ("Stop", "stop"),
            _ => ("Start", "start"),
        };
        self.confirm = Some(Confirm {
            message: format!("{verb} {} “{}”?", group.trim_end_matches('s'), name),
            args: vec![group.to_string(), action.to_string(), id],
        });
    }

    pub fn cancel_confirm(&mut self) {
        self.confirm = None;
    }

    pub fn confirm_execute(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(c) = self.confirm.take() else {
            return;
        };
        let base = c.message.trim_end_matches('?').to_string();
        self.flash = Some((format!("⏳ {base}…"), Instant::now()));

        let (tx, rx) = oneshot::channel();
        self.action_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let args: Vec<&str> = c.args.iter().map(String::as_str).collect();
            let result = match cli.run_action(&args).await {
                Ok(()) => Ok(format!("✓ {base} — done")),
                Err(e) => Err(format!("✗ {e:#}")),
            };
            let _ = tx.send(result);
        });
    }

    /// Applies a finished action; refreshes on success. Returns true on change.
    pub fn poll_action(&mut self, cli: &Arc<DatabricksCli>) -> bool {
        let Some(rx) = &mut self.action_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                let ok = result.is_ok();
                self.flash = Some((result.unwrap_or_else(|e| e), Instant::now()));
                self.action_rx = None;
                if ok {
                    self.start_refresh(cli);
                }
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.action_rx = None;
                true
            }
        }
    }

    /// Drops the flash message once it has been visible long enough.
    pub fn expire_flash(&mut self) -> bool {
        if let Some((_, since)) = &self.flash {
            if since.elapsed() >= Duration::from_secs(5) && self.action_rx.is_none() {
                self.flash = None;
                return true;
            }
        }
        false
    }

    /// Opens the selected item (or the open detail view) in the workspace web UI.
    pub fn open_in_browser(&self) {
        let Some(host) = &self.host else {
            return;
        };
        let (panel, id) = match &self.detail {
            Some(d) => (d.panel, Some(d.id.clone())),
            None => (self.focus, self.selected_item().and_then(|i| i.id.clone())),
        };
        let Some(id) = id else {
            return;
        };
        let path = match panel {
            Panel::Clusters => format!("compute/clusters/{id}"),
            Panel::Jobs => format!("jobs/{id}"),
            Panel::Pipelines => format!("pipelines/{id}"),
            Panel::Warehouses => format!("sql/warehouses/{id}"),
        };
        let url = format!("{}/{}", host.trim_end_matches('/'), path);
        #[cfg(target_os = "macos")]
        let opener = "open";
        #[cfg(not(target_os = "macos"))]
        let opener = "xdg-open";
        let _ = std::process::Command::new(opener).arg(url).spawn();
    }

    /// Counts of (ok, pending, failed, idle) items across all panels.
    pub fn status_counts(&self) -> (usize, usize, usize, usize) {
        let (mut ok, mut pending, mut failed, mut idle) = (0, 0, 0, 0);
        for shape in self.shapes.iter().flatten() {
            if let Shape::List(items) = shape {
                for item in items {
                    match item.status {
                        Status::Running | Status::Success => ok += 1,
                        Status::Pending => pending += 1,
                        Status::Failed => failed += 1,
                        Status::Stopped => idle += 1,
                        Status::Unknown(_) => {}
                    }
                }
            }
        }
        (ok, pending, failed, idle)
    }

    pub fn last_refresh_age(&self) -> Duration {
        self.last_refresh.elapsed()
    }

    pub fn spinner(&self) -> &'static str {
        SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()]
    }

    /// True whenever any background work is in flight — the loop uses this
    /// to keep spinners ticking, not just during panel refreshes.
    pub fn busy(&self) -> bool {
        self.loading || self.detail_rx.is_some() || self.action_rx.is_some()
    }

    pub fn tick_spinner(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    pub fn toggle_zoom(&mut self) {
        self.zoomed = !self.zoomed;
    }

    pub fn focus_next(&mut self) {
        let idx = Panel::ALL
            .iter()
            .position(|p| p == &self.focus)
            .unwrap_or(0);
        self.focus = Panel::ALL[(idx + 1) % Panel::ALL.len()];
    }

    pub fn focus_prev(&mut self) {
        let idx = Panel::ALL
            .iter()
            .position(|p| p == &self.focus)
            .unwrap_or(0);
        self.focus = Panel::ALL[(idx + Panel::ALL.len() - 1) % Panel::ALL.len()];
    }

    pub fn needs_refresh(&self) -> bool {
        !self.loading && self.last_refresh.elapsed() >= self.refresh_interval
    }

    pub fn start_refresh(&mut self, cli: &Arc<DatabricksCli>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        self.last_refresh = Instant::now();

        let (tx, rx) = mpsc::unbounded_channel();
        self.pending = Some(rx);
        self.in_flight = 5;

        // One task per source so each panel updates as soon as its fetch lands,
        // instead of waiting for the slowest of the five.
        macro_rules! spawn_fetch {
            ($update:expr, $fetch:path) => {{
                let cli = Arc::clone(cli);
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = $fetch(&cli).await.map_err(|e| format!("{e:#}"));
                    let _ = tx.send($update(result));
                });
            }};
        }

        spawn_fetch!(|s| Update::Panel(0, s), fetchers::clusters::fetch);
        spawn_fetch!(|s| Update::Panel(1, s), fetchers::jobs::fetch);
        spawn_fetch!(|s| Update::Panel(2, s), fetchers::pipelines::fetch);
        spawn_fetch!(|s| Update::Panel(3, s), fetchers::warehouses::fetch);
        spawn_fetch!(
            |s: Result<Shape, String>| Update::Badge(s.ok()),
            fetchers::current_user::fetch
        );
    }

    /// Applies any fetch results that have arrived; returns true if the UI should redraw.
    pub fn poll_refresh(&mut self) -> bool {
        let Some(rx) = &mut self.pending else {
            return false;
        };
        let mut changed = false;
        loop {
            match rx.try_recv() {
                Ok(Update::Panel(i, result)) => {
                    match result {
                        Ok(shape) => self.shapes[i] = Some(shape),
                        // Keep previous data on failure so panels don't blank
                        // out — but surface the error if there's nothing yet.
                        Err(e) => {
                            if matches!(self.shapes[i], None | Some(Shape::Text(_))) {
                                self.shapes[i] = Some(Shape::Text(format!("✗ {e}")));
                            }
                        }
                    }
                    self.in_flight -= 1;
                    changed = true;
                }
                Ok(Update::Badge(badge)) => {
                    if badge.is_some() {
                        self.user_badge = badge;
                    }
                    self.in_flight -= 1;
                    changed = true;
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.in_flight = 0;
                    break;
                }
            }
        }
        if self.in_flight == 0 {
            self.loading = false;
            self.pending = None;
            changed = true;
        }
        changed
    }
}
