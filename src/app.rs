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
    Dashboards,
    Catalog,
}

impl Panel {
    pub const ALL: &'static [Panel] = &[
        Panel::Clusters,
        Panel::Jobs,
        Panel::Pipelines,
        Panel::Warehouses,
        Panel::Dashboards,
        Panel::Catalog,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Panel::Clusters => "Compute",
            Panel::Jobs => "Lakeflow Jobs",
            Panel::Pipelines => "Lakeflow Pipelines",
            Panel::Warehouses => "SQL Warehouses",
            Panel::Dashboards => "AI/BI Dashboards",
            Panel::Catalog => "Unity Catalog",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Panel::Clusters => "◆",
            Panel::Jobs => "◈",
            Panel::Pipelines => "⇶",
            Panel::Warehouses => "▣",
            Panel::Dashboards => "▤",
            Panel::Catalog => "◫",
        }
    }

    /// The databricks CLI command group whose `get <id>` returns item details.
    pub fn cli_group(&self) -> &'static str {
        match self {
            Panel::Clusters => "clusters",
            Panel::Jobs => "jobs",
            Panel::Pipelines => "pipelines",
            Panel::Warehouses => "warehouses",
            Panel::Dashboards => "lakeview",
            Panel::Catalog => "tables",
        }
    }
}

pub struct Detail {
    pub panel: Panel,
    pub name: String,
    pub id: String,
    /// Item kind for Unity Catalog leaves (TABLE / VIEW / VOLUME).
    pub kind: Option<String>,
    /// Heading of the activity section ("Recent activity", "Access", ...).
    pub section: &'static str,
    /// None while the fetch is in flight.
    pub data: Option<DetailData>,
    /// Toggles between the formatted summary and the raw JSON.
    pub show_raw: bool,
    pub scroll: u16,
}

/// Full-screen sample-data view for a Unity Catalog table or view.
pub struct Preview {
    pub name: String,
    /// Display name and id of the warehouse running the query.
    pub warehouse: String,
    pub warehouse_id: String,
    /// None while the query runs; then rows or an error.
    pub data: Option<Result<crate::shape::TableData, String>>,
    pub scroll: usize,
}

/// What a confirmed warehouse choice should run.
enum PickTarget {
    Preview(String),
    Cost,
}

/// Overlay for choosing which SQL warehouse runs a query.
pub struct WhPicker {
    pub index: usize,
    target: PickTarget,
}

/// Full-screen DBU usage view backed by system.billing.usage.
pub struct CostView {
    pub warehouse: String,
    pub data: Option<Result<fetchers::cost::CostData, String>>,
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
    pub selected: [usize; 6],
    pub host: Option<String>,
    /// Available profiles from ~/.databrickscfg and the active one.
    pub profiles: Vec<String>,
    pub profile: Option<String>,
    /// When Some, the workspace picker overlay is open at this index.
    pub picker: Option<usize>,
    /// Current position in the Unity Catalog tree: [], [catalog] or [catalog, schema].
    pub uc_path: Vec<String>,
    uc_rx: Option<oneshot::Receiver<Result<Shape, String>>>,
    pub preview: Option<Preview>,
    preview_rx: Option<oneshot::Receiver<Result<crate::shape::TableData, String>>>,
    pub wh_picker: Option<WhPicker>,
    /// Session-remembered (id, name) of the warehouse used for previews.
    pub preview_warehouse: Option<(String, String)>,
    pub cost: Option<CostView>,
    cost_rx: Option<oneshot::Receiver<Result<fetchers::cost::CostData, String>>>,
    pending: Option<mpsc::UnboundedReceiver<Update>>,
    detail_rx: Option<oneshot::Receiver<DetailData>>,
    action_rx: Option<oneshot::Receiver<Result<String, String>>>,
    host_rx: Option<oneshot::Receiver<Option<String>>>,
    in_flight: usize,
    spinner_frame: usize,
    /// Splash screen deadline; None once dismissed.
    pub splash_until: Option<Instant>,
    /// When each pane last received fresh data — drives the title flash.
    pub updated_at: [Option<Instant>; 6],
}

impl App {
    pub fn new(refresh_secs: u64, theme: ThemeMode) -> Self {
        Self {
            focus: Panel::Clusters,
            theme,
            zoomed: false,
            shapes: vec![None; 6],
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
            selected: [0; 6],
            host: None,
            profiles: Vec::new(),
            profile: None,
            picker: None,
            uc_path: Vec::new(),
            uc_rx: None,
            preview: None,
            preview_rx: None,
            wh_picker: None,
            preview_warehouse: None,
            cost: None,
            cost_rx: None,
            pending: None,
            detail_rx: None,
            action_rx: None,
            host_rx: None,
            in_flight: 0,
            spinner_frame: 0,
            splash_until: Some(Instant::now() + Duration::from_millis(1600)),
            updated_at: [None; 6],
        }
    }

    pub fn splash_active(&self) -> bool {
        self.splash_until
            .map(|t| Instant::now() < t)
            .unwrap_or(false)
    }

    pub fn dismiss_splash(&mut self) {
        self.splash_until = None;
    }

    /// True while any pane's data just landed — keeps the flash decaying.
    pub fn any_fresh(&self) -> bool {
        self.updated_at
            .iter()
            .flatten()
            .any(|t| t.elapsed() < Duration::from_millis(1200))
    }

    pub fn open_picker(&mut self) {
        if self.profiles.is_empty() {
            return;
        }
        let current = self
            .profile
            .as_deref()
            .and_then(|p| self.profiles.iter().position(|n| n == p))
            .unwrap_or(0);
        self.picker = Some(current);
    }

    pub fn picker_next(&mut self) {
        if let Some(i) = self.picker {
            self.picker = Some((i + 1).min(self.profiles.len().saturating_sub(1)));
        }
    }

    pub fn picker_prev(&mut self) {
        if let Some(i) = self.picker {
            self.picker = Some(i.saturating_sub(1));
        }
    }

    /// Confirms the picker selection; returns the new CLI handle to use.
    pub fn picker_select(&mut self) -> Option<Arc<DatabricksCli>> {
        let idx = self.picker.take()?;
        let name = self.profiles.get(idx)?.clone();
        let profile_arg = if name == "DEFAULT" {
            None
        } else {
            Some(name.clone())
        };
        self.profile = Some(name);

        // Drop all workspace-specific state; panes go back to loading.
        self.shapes = vec![None; 6];
        self.user_badge = None;
        self.host = None;
        self.selected = [0; 6];
        self.detail = None;
        self.detail_rx = None;
        self.confirm = None;
        self.uc_path.clear();
        self.uc_rx = None;
        self.preview = None;
        self.preview_rx = None;
        self.wh_picker = None;
        self.preview_warehouse = None;
        self.cost = None;
        self.cost_rx = None;
        self.pending = None;
        self.in_flight = 0;
        self.loading = false;
        self.zoomed = false;

        Some(Arc::new(DatabricksCli::new(profile_arg)))
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
        let kind = match &item.status {
            Status::Unknown(k) if !k.is_empty() => Some(k.clone()),
            _ => None,
        };
        let section = match self.focus {
            Panel::Dashboards => "Contents",
            Panel::Catalog => "Columns",
            Panel::Warehouses => "Recent queries",
            _ => "Recent activity",
        };
        self.detail = Some(Detail {
            panel: self.focus,
            name: item.name.clone(),
            id: id.clone(),
            kind,
            section,
            data: None,
            show_raw: false,
            scroll: 0,
        });

        let (tx, rx) = oneshot::channel();
        self.detail_rx = Some(rx);
        let cli = Arc::clone(cli);
        let group = match &self.detail.as_ref().unwrap().kind {
            Some(k) if k == "VOLUME" => "volumes",
            _ => self.focus.cli_group(),
        };
        tokio::spawn(async move {
            let data = fetchers::detail::fetch(&cli, group, &id).await;
            let _ = tx.send(data);
        });
    }

    /// Descends one level in the Unity Catalog tree. Returns false when the
    /// selection is a leaf (caller should open the detail view instead).
    pub fn uc_drill(&mut self, cli: &Arc<DatabricksCli>) -> bool {
        if self.focus != Panel::Catalog || self.uc_path.len() >= 2 {
            return false;
        }
        let Some(item) = self.selected_item() else {
            return true; // empty pane: swallow the key
        };
        self.uc_path.push(item.name.clone());
        self.refresh_catalog(cli);
        true
    }

    /// Ascends one level; returns false if already at the catalog root.
    pub fn uc_up(&mut self, cli: &Arc<DatabricksCli>) -> bool {
        if self.focus != Panel::Catalog || self.uc_path.is_empty() {
            return false;
        }
        self.uc_path.pop();
        self.refresh_catalog(cli);
        true
    }

    fn refresh_catalog(&mut self, cli: &Arc<DatabricksCli>) {
        self.shapes[5] = None;
        self.selected[5] = 0;
        let (tx, rx) = oneshot::channel();
        self.uc_rx = Some(rx);
        let cli = Arc::clone(cli);
        let path = self.uc_path.clone();
        tokio::spawn(async move {
            let result = fetchers::catalog::fetch(&cli, &path)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
        });
    }

    /// All known warehouses as (name, id, running).
    pub fn warehouses(&self) -> Vec<(String, String, bool)> {
        let Some(Shape::List(items)) = &self.shapes[3] else {
            return Vec::new();
        };
        items
            .iter()
            .filter_map(|i| {
                let id = i.id.clone()?;
                Some((i.name.clone(), id, matches!(i.status, Status::Running)))
            })
            .collect()
    }

    /// Runs a sample-data query for the selected table or view. With
    /// `force_pick` (or several warehouses and no remembered choice) a
    /// warehouse picker opens first.
    pub fn open_preview(&mut self, cli: &Arc<DatabricksCli>, force_pick: bool) {
        if self.focus != Panel::Catalog {
            return;
        }
        let Some(item) = self.selected_item() else {
            return;
        };
        if !matches!(&item.status, Status::Unknown(k) if k == "TABLE" || k == "VIEW") {
            return;
        }
        let Some(full_name) = item.id.clone() else {
            return;
        };
        let warehouses = self.warehouses();
        if warehouses.is_empty() {
            self.flash = Some((
                "✗ no SQL warehouse available for previews".to_string(),
                Instant::now(),
            ));
            return;
        }
        if !force_pick {
            if let Some((id, name)) = self.preview_warehouse.clone() {
                self.start_preview_query(cli, full_name, id, name);
                return;
            }
            if let [(name, id, _)] = warehouses.as_slice() {
                self.preview_warehouse = Some((id.clone(), name.clone()));
                self.start_preview_query(cli, full_name, id.clone(), name.clone());
                return;
            }
        }
        // Default the cursor to the remembered choice, else a running warehouse.
        let index = self
            .preview_warehouse
            .as_ref()
            .and_then(|(id, _)| warehouses.iter().position(|(_, wid, _)| wid == id))
            .or_else(|| warehouses.iter().position(|(_, _, running)| *running))
            .unwrap_or(0);
        self.wh_picker = Some(WhPicker {
            index,
            target: PickTarget::Preview(full_name),
        });
    }

    /// Opens the DBU usage view, resolving a warehouse like previews do.
    pub fn open_cost(&mut self, cli: &Arc<DatabricksCli>) {
        let warehouses = self.warehouses();
        if warehouses.is_empty() {
            self.flash = Some((
                "✗ no SQL warehouse available to query system tables".to_string(),
                Instant::now(),
            ));
            return;
        }
        if let Some((id, name)) = self.preview_warehouse.clone() {
            self.start_cost_query(cli, id, name);
            return;
        }
        if let [(name, id, _)] = warehouses.as_slice() {
            self.preview_warehouse = Some((id.clone(), name.clone()));
            self.start_cost_query(cli, id.clone(), name.clone());
            return;
        }
        let index = warehouses
            .iter()
            .position(|(_, _, running)| *running)
            .unwrap_or(0);
        self.wh_picker = Some(WhPicker {
            index,
            target: PickTarget::Cost,
        });
    }

    fn start_cost_query(&mut self, cli: &Arc<DatabricksCli>, id: String, name: String) {
        self.cost = Some(CostView {
            warehouse: name,
            data: None,
        });
        let (tx, rx) = oneshot::channel();
        self.cost_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let result = fetchers::cost::fetch(&cli, &id).await;
            let _ = tx.send(result);
        });
    }

    pub fn close_cost(&mut self) {
        self.cost = None;
        self.cost_rx = None;
    }

    pub fn poll_cost(&mut self) -> bool {
        let Some(rx) = &mut self.cost_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                if result.is_err() {
                    self.preview_warehouse = None;
                }
                if let Some(cv) = &mut self.cost {
                    cv.data = Some(result);
                }
                self.cost_rx = None;
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.cost_rx = None;
                true
            }
        }
    }

    pub fn wh_picker_next(&mut self) {
        let len = self.warehouses().len();
        if let Some(p) = &mut self.wh_picker {
            p.index = (p.index + 1).min(len.saturating_sub(1));
        }
    }

    pub fn wh_picker_prev(&mut self) {
        if let Some(p) = &mut self.wh_picker {
            p.index = p.index.saturating_sub(1);
        }
    }

    pub fn wh_picker_cancel(&mut self) {
        self.wh_picker = None;
    }

    /// Confirms the warehouse choice, remembers it, and starts the preview.
    pub fn wh_picker_select(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(picker) = self.wh_picker.take() else {
            return;
        };
        let warehouses = self.warehouses();
        let Some((name, id, _)) = warehouses.get(picker.index) else {
            return;
        };
        self.preview_warehouse = Some((id.clone(), name.clone()));
        match picker.target {
            PickTarget::Preview(table) => {
                self.start_preview_query(cli, table, id.clone(), name.clone())
            }
            PickTarget::Cost => self.start_cost_query(cli, id.clone(), name.clone()),
        }
    }

    fn start_preview_query(
        &mut self,
        cli: &Arc<DatabricksCli>,
        full_name: String,
        warehouse_id: String,
        warehouse_name: String,
    ) {
        self.preview = Some(Preview {
            name: full_name.clone(),
            warehouse: warehouse_name,
            warehouse_id: warehouse_id.clone(),
            data: None,
            scroll: 0,
        });
        let (tx, rx) = oneshot::channel();
        self.preview_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let result = fetchers::preview::fetch(&cli, &full_name, &warehouse_id).await;
            let _ = tx.send(result);
        });
    }

    pub fn close_preview(&mut self) {
        self.preview = None;
        self.preview_rx = None;
    }

    pub fn poll_preview(&mut self) -> bool {
        let Some(rx) = &mut self.preview_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                // A warehouse that errors shouldn't stay the session default.
                if result.is_err() {
                    self.preview_warehouse = None;
                }
                if let Some(pv) = &mut self.preview {
                    pv.data = Some(result);
                }
                self.preview_rx = None;
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.preview_rx = None;
                true
            }
        }
    }

    pub fn preview_scroll(&mut self, delta: i32) {
        if let Some(pv) = &mut self.preview {
            let max = match &pv.data {
                Some(Ok(t)) => t.rows.len().saturating_sub(1),
                _ => 0,
            };
            pv.scroll = if delta < 0 {
                pv.scroll.saturating_sub(delta.unsigned_abs() as usize)
            } else {
                (pv.scroll + delta as usize).min(max)
            };
        }
    }

    pub fn poll_uc(&mut self) -> bool {
        let Some(rx) = &mut self.uc_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.shapes[5] = Some(match result {
                    Ok(shape) => shape,
                    Err(e) => Shape::Text(format!("✗ {e}")),
                });
                self.updated_at[5] = Some(Instant::now());
                self.uc_rx = None;
                true
            }
            Err(oneshot::error::TryRecvError::Empty) => false,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.uc_rx = None;
                true
            }
        }
    }

    /// Opens the access view for the selected item: effective UC grants
    /// or the workspace object ACL.
    pub fn open_grants(&mut self, cli: &Arc<DatabricksCli>) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let Some(id) = item.id.clone() else {
            return;
        };
        let (uc, object_type): (bool, &'static str) = match self.focus {
            Panel::Catalog => match &item.status {
                Status::Unknown(k) if k == "CATALOG" => (true, "catalog"),
                Status::Unknown(k) if k == "SCHEMA" => (true, "schema"),
                Status::Unknown(k) if k == "TABLE" || k == "VIEW" => (true, "table"),
                Status::Unknown(k) if k == "VOLUME" => (true, "volume"),
                _ => return,
            },
            Panel::Clusters => (false, "clusters"),
            Panel::Jobs => (false, "jobs"),
            Panel::Pipelines => (false, "pipelines"),
            Panel::Warehouses => (false, "warehouses"),
            Panel::Dashboards => (false, "dashboards"),
        };
        self.detail = Some(Detail {
            panel: self.focus,
            name: item.name.clone(),
            id: id.clone(),
            kind: None,
            section: "Access",
            data: None,
            show_raw: false,
            scroll: 0,
        });
        let (tx, rx) = oneshot::channel();
        self.detail_rx = Some(rx);
        let cli = Arc::clone(cli);
        tokio::spawn(async move {
            let data = fetchers::grants::fetch(&cli, uc, object_type, &id).await;
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
        // Dashboards and Unity Catalog objects have no start/stop/run semantics.
        if matches!(self.focus, Panel::Dashboards | Panel::Catalog) {
            return;
        }
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
            Panel::Dashboards => format!("sql/dashboardsv3/{id}"),
            Panel::Catalog => format!("explore/data/{}", id.replace('.', "/")),
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

    pub fn spinner_frame(&self) -> usize {
        self.spinner_frame
    }

    /// True whenever any background work is in flight — the loop uses this
    /// to keep spinners ticking, not just during panel refreshes.
    pub fn busy(&self) -> bool {
        self.loading
            || self.detail_rx.is_some()
            || self.action_rx.is_some()
            || self.preview_rx.is_some()
            || self.cost_rx.is_some()
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
        self.in_flight = 7;

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
        spawn_fetch!(|s| Update::Panel(4, s), fetchers::dashboards::fetch);
        spawn_fetch!(
            |s: Result<Shape, String>| Update::Badge(s.ok()),
            fetchers::current_user::fetch
        );
        {
            let cli = Arc::clone(cli);
            let tx = tx.clone();
            let path = self.uc_path.clone();
            tokio::spawn(async move {
                let result = fetchers::catalog::fetch(&cli, &path)
                    .await
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(Update::Panel(5, result));
            });
        }
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
                        Ok(shape) => {
                            self.shapes[i] = Some(shape);
                            self.updated_at[i] = Some(Instant::now());
                        }
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
