use crate::app::{App, Panel, ThemeMode};
use crate::shape::{DetailData, Shape, Status, TableData};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Padding, Paragraph,
        Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, Wrap,
    },
    Frame,
};
use std::time::Duration;

struct Palette {
    text: Color,
    dim: Color,
    border: Color,
    warn: Color,
    ok: Color,
    err: Color,
    key: Color,
    brand: Color,
    clusters: Color,
    jobs: Color,
    pipelines: Color,
    warehouses: Color,
    catalog: Color,
    /// Header wordmark gradient endpoints.
    grad_from: (u8, u8, u8),
    grad_to: (u8, u8, u8),
}

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgb3(hex: u32) -> (u8, u8, u8) {
    ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

fn palette(mode: ThemeMode) -> Palette {
    match mode {
        // Dark theme sticks to ANSI colors so it follows the terminal's own scheme.
        ThemeMode::Dark => Palette {
            text: Color::White,
            dim: Color::DarkGray,
            border: Color::DarkGray,
            warn: Color::Yellow,
            ok: Color::Green,
            err: Color::Red,
            key: Color::Cyan,
            brand: Color::Red,
            clusters: Color::Cyan,
            jobs: Color::Magenta,
            pipelines: Color::Green,
            warehouses: Color::Blue,
            catalog: rgb(0xFF8C42),
            grad_from: rgb3(0xFF3621),
            grad_to: rgb3(0xFFA046),
        },
        // Light theme uses explicit darker shades that stay readable on a white background.
        ThemeMode::Light => Palette {
            text: Color::Black,
            dim: rgb(0x6B7280),
            border: rgb(0x9CA3AF),
            warn: rgb(0xB45309),
            ok: rgb(0x15803D),
            err: rgb(0xB91C1C),
            key: rgb(0x0891B2),
            brand: rgb(0xDC2626),
            clusters: rgb(0x0891B2),
            jobs: rgb(0xA21CAF),
            pipelines: rgb(0x15803D),
            warehouses: rgb(0x1D4ED8),
            catalog: rgb(0xC2410C),
            grad_from: rgb3(0xB91C1C),
            grad_to: rgb3(0xC2410C),
        },
        ThemeMode::CatppuccinMocha => Palette {
            text: rgb(0xCDD6F4),
            dim: rgb(0x6C7086),
            border: rgb(0x585B70),
            warn: rgb(0xF9E2AF),
            ok: rgb(0xA6E3A1),
            err: rgb(0xF38BA8),
            key: rgb(0x89DCEB),
            brand: rgb(0xF38BA8),
            clusters: rgb(0x89DCEB),
            jobs: rgb(0xCBA6F7),
            pipelines: rgb(0xA6E3A1),
            warehouses: rgb(0x89B4FA),
            catalog: rgb(0xFAB387),
            grad_from: rgb3(0xF38BA8),
            grad_to: rgb3(0xFAB387),
        },
        ThemeMode::CatppuccinLatte => Palette {
            text: rgb(0x4C4F69),
            dim: rgb(0x8C8FA1),
            border: rgb(0xACB0BE),
            warn: rgb(0xDF8E1D),
            ok: rgb(0x40A02B),
            err: rgb(0xD20F39),
            key: rgb(0x04A5E5),
            brand: rgb(0xD20F39),
            clusters: rgb(0x04A5E5),
            jobs: rgb(0x8839EF),
            pipelines: rgb(0x40A02B),
            warehouses: rgb(0x1E66F5),
            catalog: rgb(0xFE640B),
            grad_from: rgb3(0xD20F39),
            grad_to: rgb3(0xFE640B),
        },
        ThemeMode::GruvboxDark => Palette {
            text: rgb(0xEBDBB2),
            dim: rgb(0x928374),
            border: rgb(0x665C54),
            warn: rgb(0xFABD2F),
            ok: rgb(0xB8BB26),
            err: rgb(0xFB4934),
            key: rgb(0x8EC07C),
            brand: rgb(0xFB4934),
            clusters: rgb(0x8EC07C),
            jobs: rgb(0xD3869B),
            pipelines: rgb(0xB8BB26),
            warehouses: rgb(0x83A598),
            catalog: rgb(0xFE8019),
            grad_from: rgb3(0xFB4934),
            grad_to: rgb3(0xFE8019),
        },
        ThemeMode::Dracula => Palette {
            text: rgb(0xF8F8F2),
            dim: rgb(0x6272A4),
            border: rgb(0x44475A),
            warn: rgb(0xF1FA8C),
            ok: rgb(0x50FA7B),
            err: rgb(0xFF5555),
            key: rgb(0x8BE9FD),
            brand: rgb(0xFF5555),
            clusters: rgb(0x8BE9FD),
            jobs: rgb(0xFF79C6),
            pipelines: rgb(0x50FA7B),
            warehouses: rgb(0xBD93F9),
            catalog: rgb(0xFFB86C),
            grad_from: rgb3(0xFF5555),
            grad_to: rgb3(0xFFB86C),
        },
        ThemeMode::Nord => Palette {
            text: rgb(0xD8DEE9),
            dim: rgb(0x4C566A),
            border: rgb(0x434C5E),
            warn: rgb(0xEBCB8B),
            ok: rgb(0xA3BE8C),
            err: rgb(0xBF616A),
            key: rgb(0x88C0D0),
            brand: rgb(0xBF616A),
            clusters: rgb(0x88C0D0),
            jobs: rgb(0xB48EAD),
            pipelines: rgb(0xA3BE8C),
            warehouses: rgb(0x81A1C1),
            catalog: rgb(0xD08770),
            grad_from: rgb3(0xBF616A),
            grad_to: rgb3(0xD08770),
        },
        ThemeMode::TokyoNight => Palette {
            text: rgb(0xC0CAF5),
            dim: rgb(0x565F89),
            border: rgb(0x3B4261),
            warn: rgb(0xE0AF68),
            ok: rgb(0x9ECE6A),
            err: rgb(0xF7768E),
            key: rgb(0x7DCFFF),
            brand: rgb(0xF7768E),
            clusters: rgb(0x7DCFFF),
            jobs: rgb(0xBB9AF7),
            pipelines: rgb(0x9ECE6A),
            warehouses: rgb(0x7AA2F7),
            catalog: rgb(0xFF9E64),
            grad_from: rgb3(0xF7768E),
            grad_to: rgb3(0xFF9E64),
        },
    }
}

fn accent(panel: Panel, p: &Palette) -> Color {
    match panel {
        Panel::Clusters => p.clusters,
        Panel::Jobs => p.jobs,
        Panel::Pipelines => p.pipelines,
        Panel::Warehouses => p.warehouses,
        Panel::Dashboards => p.key,
        Panel::Catalog => p.catalog,
        Panel::Secrets => p.warn,
    }
}

fn pane_breadcrumb(app: &App, panel: Panel) -> String {
    match panel {
        Panel::Catalog => app.uc_path.join("."),
        Panel::Secrets => app.secret_scope.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    let p = palette(app.theme);

    if app.splash_active() {
        draw_splash(f, f.area(), app, &p);
        return;
    }

    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_header(f, root[0], app, &p);
    draw_footer(f, root[2], app, &p);

    if app.sql.is_some() {
        draw_sql(f, root[1], app, &p);
        if app.sql_complete.is_some() {
            draw_sql_complete(f, root[1], app, &p);
        }
        // Running a query may pop the warehouse picker over the console.
        if app.wh_picker.is_some() {
            draw_wh_picker(f, root[1], app, &p);
        }
        return;
    }

    if app.cost.is_some() {
        draw_cost(f, root[1], app, &p);
        return;
    }

    if app.preview.is_some() {
        draw_preview(f, root[1], app, &p);
        return;
    }

    if app.run_view.is_some() {
        draw_run(f, root[1], app, &p);
        return;
    }

    if app.detail.is_some() {
        draw_detail(f, root[1], app, &p);
        return;
    }

    if app.zoomed {
        let idx = Panel::ALL
            .iter()
            .position(|pn| pn == &app.focus)
            .unwrap_or(0);
        draw_panel(
            f,
            root[1],
            app.focus,
            app.shapes[idx].as_ref(),
            true,
            Some(app.selection(idx)),
            false,
            &pane_breadcrumb(app, app.focus),
            app.spinner(),
            &app.filters[idx],
            app.filter_entry,
            &p,
        );
        // Overlays must still render on top of the zoomed pane.
        if app.picker.is_some() {
            draw_picker(f, root[1], app, &p);
        }
        if app.wh_picker.is_some() {
            draw_wh_picker(f, root[1], app, &p);
        }
        if app.problems.is_some() {
            draw_problems(f, root[1], app, &p);
        }
        if app.upcoming.is_some() {
            draw_upcoming(f, root[1], app, &p);
        }
        if app.jump.is_some() {
            draw_jump(f, root[1], app, &p);
        }
        if app.pane_cfg.is_some() {
            draw_pane_cfg(f, root[1], app, &p);
        }
        if app.secret_form.is_some() {
            draw_secret_form(f, root[1], app, &p);
        }
        if app.help {
            draw_help(f, root[1], app, &p);
        }
        return;
    }

    // The grid adapts to however many panes are visible, in the user's
    // order: left column gets the first half (rounded up), right the rest.
    let visible = app.visible_panes();
    if visible.is_empty() {
        let par = Paragraph::new("all panes hidden — press H to bring them back")
            .style(Style::default().fg(p.dim))
            .alignment(Alignment::Center);
        f.render_widget(par, root[1]);
    } else {
        let rows_of = |n: usize| -> Vec<Constraint> {
            (0..n).map(|_| Constraint::Ratio(1, n as u32)).collect()
        };
        let left_count = visible.len().div_ceil(2);
        let right_count = visible.len() - left_count;
        let mut areas: Vec<Rect> = Vec::new();
        if right_count == 0 {
            areas.extend(
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(rows_of(left_count))
                    .split(root[1])
                    .iter(),
            );
        } else {
            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(root[1]);
            areas.extend(
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(rows_of(left_count))
                    .split(body[0])
                    .iter(),
            );
            areas.extend(
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(rows_of(right_count))
                    .split(body[1])
                    .iter(),
            );
        }

        for (slot, &i) in visible.iter().enumerate() {
            let panel = Panel::ALL[i];
            let focused = app.focus == panel;
            let shape = app.shapes[i].as_ref();
            let selected = focused.then(|| app.selection(i));
            let fresh = app.updated_at[i]
                .map(|t| t.elapsed() < Duration::from_millis(1200))
                .unwrap_or(false);
            draw_panel(
                f,
                areas[slot],
                panel,
                shape,
                focused,
                selected,
                fresh,
                &pane_breadcrumb(app, panel),
                app.spinner(),
                &app.filters[i],
                app.filter_entry && focused,
                &p,
            );
        }
    }

    if app.picker.is_some() {
        draw_picker(f, root[1], app, &p);
    }
    if app.wh_picker.is_some() {
        draw_wh_picker(f, root[1], app, &p);
    }
    if app.problems.is_some() {
        draw_problems(f, root[1], app, &p);
    }
    if app.upcoming.is_some() {
        draw_upcoming(f, root[1], app, &p);
    }
    if app.jump.is_some() {
        draw_jump(f, root[1], app, &p);
    }
    if app.pane_cfg.is_some() {
        draw_pane_cfg(f, root[1], app, &p);
    }
    if app.secret_form.is_some() {
        draw_secret_form(f, root[1], app, &p);
    }
    if app.help {
        draw_help(f, root[1], app, &p);
    }
}

fn draw_secret_form(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let Some(form) = &app.secret_form else {
        return;
    };
    let width = 60.min(area.width.saturating_sub(4));
    let height = 6.min(area.height);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup);
    let title = match &form.scope {
        None => "New secret scope ".to_string(),
        Some(s) => format!("New secret in {s} "),
    };
    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" ◈ ", Style::default().fg(p.warn)),
            Span::styled(
                title,
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(p.warn).add_modifier(Modifier::BOLD))
        .padding(Padding::new(1, 1, 1, 0));
    let label0 = if form.scope.is_none() {
        "name "
    } else {
        "key  "
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(label0, Style::default().fg(p.dim)),
        Span::styled(form.key.as_str(), Style::default().fg(p.text)),
        if form.stage == 0 {
            Span::styled("▏", Style::default().fg(p.warn))
        } else {
            Span::raw("")
        },
    ])];
    if form.scope.is_some() {
        // The value is never echoed — bullets only.
        lines.push(Line::from(vec![
            Span::styled("value ", Style::default().fg(p.dim)),
            Span::styled(
                "•".repeat(form.value.chars().count()),
                Style::default().fg(p.text),
            ),
            if form.stage == 1 {
                Span::styled("▏", Style::default().fg(p.warn))
            } else {
                Span::raw("")
            },
        ]));
    }
    f.render_widget(Paragraph::new(lines).block(block), popup);
}

fn draw_pane_cfg(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let Some(selected) = app.pane_cfg else {
        return;
    };
    let width = 52.min(area.width.saturating_sub(4));
    let height = 11.min(area.height);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" ▦ ", Style::default().fg(p.key)),
            Span::styled(
                "Panes ",
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(p.key).add_modifier(Modifier::BOLD))
        .padding(Padding::new(1, 1, 1, 0));
    let items: Vec<ListItem> = app
        .pane_order
        .iter()
        .map(|&i| {
            let panel = Panel::ALL[i];
            let (mark, style) = if app.hidden[i] {
                ("○ ", Style::default().fg(p.dim))
            } else {
                ("● ", Style::default().fg(p.ok))
            };
            ListItem::new(Line::from(vec![
                Span::styled(mark, style),
                Span::styled(
                    format!("{} ", panel.icon()),
                    Style::default().fg(accent(panel, p)),
                ),
                Span::styled(
                    panel.title(),
                    if app.hidden[i] {
                        Style::default().fg(p.dim)
                    } else {
                        Style::default().fg(p.text)
                    },
                ),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(Some(selected));
    f.render_stateful_widget(list, popup, &mut state);
}

fn draw_help(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let width = 74.min(area.width.saturating_sub(4));
    let height = area.height.saturating_sub(2);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + 1,
        width,
        height,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" ? ", Style::default().fg(p.key)),
            Span::styled(
                "Keys ",
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(p.key).add_modifier(Modifier::BOLD))
        .padding(Padding::new(1, 1, 1, 0));

    let sections: &[(&str, &[(&str, &str)])] = &[
        (
            "Global",
            &[
                ("tab / l, shift+tab / h", "focus next / previous pane"),
                ("j / k", "select item"),
                ("enter", "open details / drill down"),
                ("/", "filter the focused pane (esc clears)"),
                ("ctrl+p", "command palette: fuzzy-jump anywhere"),
                ("!", "problems: everything failing, enter jumps"),
                ("u", "upcoming runs: what fires next, soonest first"),
                (":", "SQL console (prefilled on a catalog table)"),
                ("$", "cost view: DBUs, dollars, top spenders"),
                ("H", "arrange panes: hide and reorder"),
                ("z", "zoom the focused pane"),
                ("w", "switch workspace profile"),
                ("t", "cycle color theme"),
                ("r", "refresh now"),
                ("?", "this help"),
                ("q / ctrl+c", "quit"),
            ],
        ),
        (
            "Focused pane",
            &[
                ("s", "start / stop / run the selected item"),
                ("g", "access: grants and permissions"),
                ("o", "open in the workspace web UI"),
            ],
        ),
        (
            "Unity Catalog pane",
            &[
                ("enter / backspace", "drill down / up (into volumes too)"),
                ("p / P", "preview table rows (P picks the warehouse)"),
                ("L", "lineage: upstream and downstream tables"),
                (":", "query the selected table"),
                ("enter on a file", "peek at its first 200 lines"),
            ],
        ),
        (
            "Details & runs",
            &[
                ("enter", "job/pipeline detail → latest run/update"),
                ("h / l", "older / newer run"),
                ("o", "full task output — keeps tailing while live"),
                ("t", "timeline: per-task Gantt of the run"),
                ("d", "dag: task dependency tree of the run"),
                ("r", "repair a failed run — reruns only failed tasks"),
                ("s", "cancel the shown run"),
                ("j / k, J", "scroll · toggle raw JSON"),
            ],
        ),
        (
            "Table previews",
            &[
                ("j / k", "scroll rows"),
                ("← / →", "page columns (wide tables)"),
                ("/", "filter columns by name"),
                ("v / enter", "record view: one row, fields stacked"),
                ("e", "export rows to CSV"),
            ],
        ),
        (
            "Secret scopes pane",
            &[
                ("enter / backspace", "open a scope / back to scopes"),
                ("a", "create a scope / add a secret (value masked)"),
                ("x", "delete the selected scope or secret"),
                ("g", "scope ACLs"),
            ],
        ),
        (
            "SQL console",
            &[
                ("enter", "run the statement"),
                ("tab", "complete catalog/schema/table/column names"),
                ("↑ / ↓, ctrl+r", "history · incremental search"),
                ("ctrl+x", "compose in $EDITOR"),
                ("ctrl+s", "export results to CSV"),
                ("pgup / pgdn", "scroll results"),
                ("shift+← / →", "page result columns"),
                ("esc", "cancel a running query, else close"),
            ],
        ),
    ];
    let mut lines: Vec<Line> = Vec::new();
    for (title, keys) in sections {
        lines.push(Line::from(Span::styled(
            *title,
            Style::default().fg(p.key).add_modifier(Modifier::BOLD),
        )));
        for (k, desc) in *keys {
            lines.push(Line::from(vec![
                Span::styled(format!("  {k:<24}"), Style::default().fg(p.warn)),
                Span::styled(*desc, Style::default().fg(p.text)),
            ]));
        }
        lines.push(Line::default());
    }
    let total = lines.len();
    let par = Paragraph::new(lines)
        .scroll((app.help_scroll, 0))
        .block(block);
    f.render_widget(par, popup);
    // Viewport loses the borders plus the block's one line of top padding.
    scrollbar(
        f,
        popup,
        total,
        popup.height.saturating_sub(3) as usize,
        app.help_scroll as usize,
        p,
    );
}

fn draw_jump(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let Some(jump) = &app.jump else {
        return;
    };
    let matches = app.jump_matches();
    let width = 70.min(area.width.saturating_sub(4));
    let height = (matches.len() as u16 + 5).min(area.height);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + 2.min(area.height.saturating_sub(height)),
        width,
        height,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" ⌕ ", Style::default().fg(p.key)),
            Span::styled(
                "Jump to ",
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(p.key).add_modifier(Modifier::BOLD))
        .padding(Padding::horizontal(1));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(inner);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(p.key)),
            Span::styled(jump.query.as_str(), Style::default().fg(p.text)),
            Span::styled("▏", Style::default().fg(p.warn)),
        ])),
        parts[0],
    );

    if matches.is_empty() {
        f.render_widget(
            Paragraph::new("∅ nothing matches").style(Style::default().fg(p.dim)),
            parts[1],
        );
        return;
    }
    let items: Vec<ListItem> = matches
        .iter()
        .map(|(panel_idx, name, label)| {
            let panel = Panel::ALL[*panel_idx];
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", panel.icon()),
                    Style::default().fg(accent(panel, p)),
                ),
                Span::styled(format!("{name}  "), Style::default().fg(p.text)),
                Span::styled(label.clone(), Style::default().fg(p.dim)),
            ]))
        })
        .collect();
    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(Some(jump.index));
    f.render_stateful_widget(list, parts[1], &mut state);
}

fn draw_problems(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let Some(pr) = &app.problems else {
        return;
    };
    let truncate = |s: &str, max: usize| -> String {
        if s.chars().count() <= max {
            s.to_string()
        } else {
            let cut: String = s.chars().take(max.saturating_sub(1)).collect();
            format!("{cut}…")
        }
    };
    // Names get a bounded column; notes (which can be whole CLI error
    // lines) take whatever room is left and are truncated to fit.
    let name_w = pr
        .items
        .iter()
        .map(|i| i.name.chars().count())
        .max()
        .unwrap_or(10)
        .clamp(10, 40);
    let note_w = pr
        .items
        .iter()
        .map(|i| i.note.chars().count())
        .max()
        .unwrap_or(0);
    // dot(2) + name + gap(2) + icon+title(20) + note + padding/borders(8)
    let width = ((name_w + note_w + 32) as u16).min(area.width.saturating_sub(4));
    let height = (pr.items.len().max(1) as u16 + 4).min(area.height);
    // Room left for the note on each row inside borders and padding.
    let note_space = (width as usize).saturating_sub(name_w + 30);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" ✗ ", Style::default().fg(p.err)),
            Span::styled(
                format!("Problems · {} ", pr.items.len()),
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.err).add_modifier(Modifier::BOLD))
        .padding(Padding::new(1, 1, 1, 1));

    if pr.items.is_empty() {
        let par = Paragraph::new("✓ all clear — nothing failing right now")
            .style(Style::default().fg(p.ok))
            .block(block);
        f.render_widget(par, popup);
        return;
    }

    let items: Vec<ListItem> = pr
        .items
        .iter()
        .map(|problem| {
            let panel = Panel::ALL[problem.panel];
            ListItem::new(Line::from(vec![
                Span::styled("✗ ", Style::default().fg(p.err)),
                Span::styled(
                    format!(
                        "{:<width$}",
                        truncate(&problem.name, name_w),
                        width = name_w + 2
                    ),
                    Style::default().fg(p.text),
                ),
                Span::styled(
                    format!("{} {:<18}", panel.icon(), panel.title()),
                    Style::default().fg(accent(panel, p)),
                ),
                Span::styled(
                    truncate(&problem.note, note_space),
                    Style::default().fg(p.dim),
                ),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(Some(pr.index));
    f.render_stateful_widget(list, popup, &mut state);
}

/// Overlay listing every job that will run again on its own, soonest
/// first: countdown, job name, and the schedule that drives it.
fn draw_upcoming(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let Some(u) = &app.upcoming else {
        return;
    };
    let label_w = u
        .items
        .iter()
        .map(|i| i.next.label.chars().count())
        .max()
        .unwrap_or(8)
        .clamp(8, 18);
    let name_w = u
        .items
        .iter()
        .map(|i| i.name.chars().count())
        .max()
        .unwrap_or(10)
        .clamp(10, 40);
    let desc_w = u
        .items
        .iter()
        .map(|i| i.next.desc.chars().count())
        .max()
        .unwrap_or(0);
    let width = ((label_w + name_w + desc_w + 12) as u16).min(area.width.saturating_sub(4));
    let height = (u.items.len().max(1) as u16 + 4).min(area.height);
    let desc_space = (width as usize).saturating_sub(label_w + name_w + 10);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" ⏱ ", Style::default().fg(p.key)),
            Span::styled(
                format!("Upcoming runs · {} ", u.items.len()),
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.key).add_modifier(Modifier::BOLD))
        .padding(Padding::new(1, 1, 1, 1));

    if u.loading {
        let par = Paragraph::new(format!("{} reading job schedules…", app.spinner()))
            .style(Style::default().fg(p.warn))
            .block(block);
        f.render_widget(par, popup);
        return;
    }
    if u.items.is_empty() {
        let par = Paragraph::new("no scheduled or triggered jobs — everything runs on demand")
            .style(Style::default().fg(p.dim))
            .block(block);
        f.render_widget(par, popup);
        return;
    }

    let truncate = |s: &str, max: usize| -> String {
        if s.chars().count() <= max {
            s.to_string()
        } else {
            let cut: String = s.chars().take(max.saturating_sub(1)).collect();
            format!("{cut}…")
        }
    };
    let items: Vec<ListItem> = u
        .items
        .iter()
        .map(|it| {
            let when_color = if it.next.paused {
                p.dim
            } else if it.next.at_ms.is_some() {
                p.ok
            } else {
                p.warn
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(
                        "{:<width$}",
                        truncate(&it.next.label, label_w),
                        width = label_w + 2
                    ),
                    Style::default().fg(when_color),
                ),
                Span::styled(
                    format!("{:<width$}", truncate(&it.name, name_w), width = name_w + 2),
                    Style::default().fg(p.text),
                ),
                Span::styled(
                    truncate(&it.next.desc, desc_space),
                    Style::default().fg(p.dim),
                ),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(Some(u.index));
    f.render_stateful_widget(list, popup, &mut state);
}

fn draw_wh_picker(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let Some(picker) = &app.wh_picker else {
        return;
    };
    let warehouses = app.warehouses();
    let name_w = warehouses
        .iter()
        .map(|(n, _, _)| n.chars().count())
        .max()
        .unwrap_or(12)
        .max(12);
    // marker(2) + dot(2) + name + gap(2) + state(7) + gap(2) + id(16) + padding/borders(6)
    let width = ((name_w + 37) as u16).min(area.width.saturating_sub(4));
    let height = (warehouses.len() as u16 + 4).min(area.height);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" ▣ ", Style::default().fg(p.warehouses)),
            Span::styled(
                "Run query on ",
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(
            Style::default()
                .fg(p.warehouses)
                .add_modifier(Modifier::BOLD),
        )
        .padding(Padding::new(1, 1, 1, 1));
    let current_id = app.preview_warehouse.as_ref().map(|(id, _)| id.as_str());
    let items: Vec<ListItem> = warehouses
        .iter()
        .map(|(name, id, running)| {
            let dot = if *running { p.ok } else { p.dim };
            let marker = if current_id == Some(id.as_str()) {
                "» "
            } else {
                "  "
            };
            let state = if *running { "running" } else { "idle   " };
            ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(p.key)),
                Span::styled("● ", Style::default().fg(dot)),
                Span::styled(format!("{name:<name_w$}"), Style::default().fg(p.text)),
                Span::styled(
                    format!("  {state}"),
                    Style::default().fg(if *running { p.ok } else { p.dim }),
                ),
                Span::styled(format!("  {id}"), Style::default().fg(p.dim)),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(Some(picker.index));
    f.render_stateful_widget(list, popup, &mut state);
}

const WORDMARK_TOP: &str = "█▀▄ ▄▀█ ▀█▀ ▄▀█ █▄▄ █▀█ █ █▀▀ █▄▀ █▀";
const WORDMARK_BOT: &str = "█▄▀ █▀█  █  █▀█ █▄█ █▀▄ █ █▄▄ █ █ ▄█";

fn draw_splash(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let lines_total = 9u16;
    let top = area.y + (area.height.saturating_sub(lines_total)) / 2;
    let frame = app.spinner_frame();

    // Brick-mark pyramid in Databricks red.
    let bricks = ["◢◤", "◢◤ ◢◤", "◢◤ ◢◤ ◢◤"];
    for (i, row) in bricks.iter().enumerate() {
        let line = Line::from(Span::styled(
            *row,
            Style::default().fg(p.brand).add_modifier(Modifier::BOLD),
        ));
        let rect = Rect {
            x: area.x,
            y: top + i as u16,
            width: area.width,
            height: 1,
        };
        f.render_widget(Paragraph::new(line).alignment(Alignment::Center), rect);
    }

    // Wordmark with a light sweep that travels across the letters.
    let sweep = (frame * 2) % (WORDMARK_TOP.chars().count() + 16);
    for (row, text) in [(4u16, WORDMARK_TOP), (5u16, WORDMARK_BOT)] {
        let spans: Vec<Span> = text
            .chars()
            .enumerate()
            .map(|(i, c)| {
                let dist = (i as i32 - sweep as i32 + 8).unsigned_abs();
                let style = if dist < 3 {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.brand)
                };
                Span::styled(c.to_string(), style)
            })
            .collect();
        let rect = Rect {
            x: area.x,
            y: top + row,
            width: area.width,
            height: 1,
        };
        f.render_widget(
            Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
            rect,
        );
    }

    let tagline = Line::from(vec![
        Span::styled("the ", Style::default().fg(p.dim)),
        Span::styled(
            "Lakehouse",
            Style::default().fg(p.key).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" in your terminal", Style::default().fg(p.dim)),
    ]);
    let version = Line::from(Span::styled(
        format!("v{}", env!("CARGO_PKG_VERSION")),
        Style::default().fg(p.dim),
    ));
    for (offset, line) in [(7u16, tagline), (8u16, version)] {
        let rect = Rect {
            x: area.x,
            y: top + offset,
            width: area.width,
            height: 1,
        };
        f.render_widget(Paragraph::new(line).alignment(Alignment::Center), rect);
    }
}

fn draw_picker(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let selected = app.picker.unwrap_or(0);
    let width = (app
        .profiles
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(10)
        .max(20) as u16
        + 6)
    .min(area.width);
    let height = (app.profiles.len() as u16 + 2).min(area.height);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(Line::from(Span::styled(
            " ⌂ Workspace ",
            Style::default().fg(p.key).add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.key))
        .padding(Padding::horizontal(1));
    let items: Vec<ListItem> = app
        .profiles
        .iter()
        .map(|name| {
            let current = app.profile.as_deref() == Some(name.as_str());
            let marker = if current { "● " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(p.ok)),
                Span::styled(name.as_str(), Style::default().fg(p.text)),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(Some(selected));
    f.render_stateful_widget(list, popup, &mut state);
}

fn bucket_color(bucket: &str, p: &Palette) -> Color {
    match bucket {
        "Jobs" => p.jobs,
        "SQL" => p.warehouses,
        "All-Purpose" => p.clusters,
        "DLT" => p.pipelines,
        _ => p.dim,
    }
}

fn draw_cost(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let cv = app.cost.as_ref().unwrap();
    let scope = match &cv.data {
        Some(Ok(d)) if d.scoped => " · this workspace",
        Some(Ok(_)) => " · all workspaces",
        _ => "",
    };
    let title = Line::from(vec![
        Span::styled(" ◢◤ ", Style::default().fg(p.brand)),
        Span::styled(
            format!("Usage · last 14 days{scope} "),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("via {} ", cv.warehouse), Style::default().fg(p.dim)),
    ]);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.brand).add_modifier(Modifier::BOLD))
        .padding(Padding::new(1, 1, 1, 1));

    match &cv.data {
        None => {
            let par = Paragraph::new(format!(
                "{} querying system.billing.usage — the warehouse may need to start…",
                app.spinner()
            ))
            .style(Style::default().fg(p.warn))
            .block(block);
            f.render_widget(par, area);
        }
        Some(Err(e)) => {
            let par = Paragraph::new(format!(
                "✗ {e}\n\nsystem tables need to be enabled and readable \
                 (grants on the `system` catalog)"
            ))
            .style(Style::default().fg(p.err))
            .wrap(Wrap { trim: false })
            .block(block);
            f.render_widget(par, area);
        }
        Some(Ok(data)) if data.days.is_empty() => {
            let par = Paragraph::new("∅ no usage recorded in the last 14 days")
                .style(Style::default().fg(p.dim))
                .block(block);
            f.render_widget(par, area);
        }
        Some(Ok(data)) => {
            let mut lines: Vec<Line> = Vec::new();

            // Legend with per-bucket totals, largest first.
            let mut legend = vec![Span::raw("")];
            for (bucket, total, usd) in &data.buckets {
                legend.push(Span::styled(
                    "■ ",
                    Style::default().fg(bucket_color(bucket, p)),
                ));
                let amount = if data.priced {
                    format!("{bucket} {total:.1} (${usd:.0})   ")
                } else {
                    format!("{bucket} {total:.1}   ")
                };
                legend.push(Span::styled(amount, Style::default().fg(p.text)));
            }
            let sigma = if data.priced {
                format!("Σ {:.1} DBU ≈ ${:.2}", data.total, data.total_usd)
            } else {
                format!("Σ {:.1} DBU", data.total)
            };
            legend.push(Span::styled(
                sigma,
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::from(legend));
            if data.priced {
                lines.push(Line::from(Span::styled(
                    " list prices before discounts",
                    Style::default().fg(p.dim),
                )));
            }
            if !data.scoped {
                lines.push(Line::from(Span::styled(
                    " couldn't resolve this workspace's id — showing the whole account",
                    Style::default().fg(p.warn),
                )));
            }
            lines.push(Line::default());

            let max_day = data
                .days
                .iter()
                .map(|d| d.total)
                .fold(0.0_f64, f64::max)
                .max(f64::EPSILON);
            let inner_w = area.width.saturating_sub(4) as usize;
            let bar_w = inner_w.saturating_sub(22).max(10);

            for day in &data.days {
                let mut spans = vec![Span::styled(
                    // "2026-07-01" -> "07-01"
                    format!("{:<6}", day.date.chars().skip(5).collect::<String>()),
                    Style::default().fg(p.dim),
                )];
                for (bucket, value) in &day.by_bucket {
                    let chars = ((value / max_day) * bar_w as f64)
                        .round()
                        .max(if *value > 0.0 { 1.0 } else { 0.0 })
                        as usize;
                    spans.push(Span::styled(
                        "█".repeat(chars),
                        Style::default().fg(bucket_color(bucket, p)),
                    ));
                }
                let day_label = if data.priced {
                    format!("  {:.1} · ${:.2}", day.total, day.total_usd)
                } else {
                    format!("  {:.1}", day.total)
                };
                spans.push(Span::styled(day_label, Style::default().fg(p.text)));
                lines.push(Line::from(spans));
            }

            if !data.spenders.is_empty() {
                lines.push(Line::default());
                lines.push(Line::from(Span::styled(
                    "TOP SPENDERS · 14 days",
                    Style::default().fg(p.dim).add_modifier(Modifier::BOLD),
                )));
                let name_w = data
                    .spenders
                    .iter()
                    .map(|s| {
                        app.resource_name(&s.kind, &s.id)
                            .unwrap_or_else(|| s.id.clone())
                            .chars()
                            .count()
                    })
                    .max()
                    .unwrap_or(0)
                    .max(8);
                for (i, s) in data.spenders.iter().enumerate() {
                    let name = app
                        .resource_name(&s.kind, &s.id)
                        .unwrap_or_else(|| s.id.clone());
                    let kind_color = match s.kind.as_str() {
                        "job" => p.jobs,
                        "cluster" => p.clusters,
                        "warehouse" => p.warehouses,
                        _ => p.dim,
                    };
                    let amount = if data.priced {
                        format!("{:>9} · {:.1} DBU", format!("${:.2}", s.usd), s.dbus)
                    } else {
                        format!("{:>8.1} DBU", s.dbus)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(format!("{:>3}. ", i + 1), Style::default().fg(p.dim)),
                        Span::styled(format!("{:<10}", s.kind), Style::default().fg(kind_color)),
                        Span::styled(
                            format!("{:<width$}", name, width = name_w + 2),
                            Style::default().fg(p.text),
                        ),
                        Span::styled(amount, Style::default().fg(p.text)),
                    ]));
                }
            }

            let par = Paragraph::new(lines).block(block);
            f.render_widget(par, area);
        }
    }
}

fn draw_sql(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let console = app.sql.as_ref().unwrap();
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let via = if console.warehouse.is_empty() {
        String::new()
    } else {
        format!("via {} ", console.warehouse)
    };
    let prompt_block = Block::default()
        .title(Line::from(vec![
            Span::styled(" ⌁ ", Style::default().fg(p.warehouses)),
            Span::styled(
                "SQL Console ",
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(via, Style::default().fg(p.dim)),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(
            Style::default()
                .fg(p.warehouses)
                .add_modifier(Modifier::BOLD),
        )
        .padding(Padding::horizontal(1));
    if let Some((query, _)) = &app.hist_search {
        // Readline-style incremental search over history.
        let matched = app
            .hist_search_current()
            .map(|s| s.replace('\n', " "))
            .unwrap_or_default();
        let prompt = Paragraph::new(Line::from(vec![
            Span::styled("(reverse-i-search)`", Style::default().fg(p.dim)),
            Span::styled(
                query.as_str(),
                Style::default().fg(p.warn).add_modifier(Modifier::BOLD),
            ),
            Span::styled("`: ", Style::default().fg(p.dim)),
            Span::styled(matched, Style::default().fg(p.text)),
        ]))
        .block(prompt_block);
        f.render_widget(prompt, parts[0]);
    } else {
        // Caret sits at the cursor, not the end; long inputs scroll so
        // the caret stays visible. Newlines from $EDITOR render as
        // spaces (byte-for-byte, so the caret math holds).
        let caret_byte = console
            .input
            .char_indices()
            .nth(console.cursor)
            .map(|(i, _)| i)
            .unwrap_or(console.input.len());
        let (before, after) = console.input.split_at(caret_byte);
        let inner_w = parts[0].width.saturating_sub(4) as usize; // borders + padding
        let hscroll = (console.cursor + 3).saturating_sub(inner_w) as u16;
        let prompt = Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(p.key)),
            Span::styled(before.replace('\n', " "), Style::default().fg(p.text)),
            Span::styled("▏", Style::default().fg(p.warn)),
            Span::styled(after.replace('\n', " "), Style::default().fg(p.text)),
        ]))
        .scroll((0, hscroll))
        .block(prompt_block);
        f.render_widget(prompt, parts[0]);
    }

    let row_info = match &console.data {
        Some(Ok(t)) => format!("{} rows ", t.rows.len()),
        _ => String::new(),
    };
    let mut results_title = vec![
        Span::styled(" Results ", Style::default().fg(p.text)),
        Span::styled(row_info, Style::default().fg(p.dim)),
    ];
    // Wide results page horizontally; say where we are.
    let avail = parts[1].width.saturating_sub(4) as usize;
    let sliced = match &console.data {
        Some(Ok(t)) if !t.headers.is_empty() => {
            let cols: Vec<usize> = (0..t.headers.len()).collect();
            let (shown, constraints) = grid_slice(t, &cols, console.col, avail);
            if shown.len() < cols.len() {
                let first = console.col.min(cols.len() - 1) + 1;
                results_title.push(Span::styled(
                    format!(
                        "· cols {}–{} of {} (⇧←/→) ",
                        first,
                        first + shown.len() - 1,
                        cols.len()
                    ),
                    Style::default().fg(p.dim),
                ));
            }
            Some((shown, constraints))
        }
        _ => None,
    };
    let results_block = Block::default()
        .title(Line::from(results_title))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.border))
        .padding(Padding::horizontal(1));

    if console.running {
        let par = Paragraph::new(format!(
            "{} running on {} — the warehouse may need a moment to start…",
            app.spinner(),
            console.warehouse
        ))
        .style(Style::default().fg(p.warn))
        .block(results_block);
        f.render_widget(par, parts[1]);
        return;
    }
    match &console.data {
        None => {
            let par = Paragraph::new(
                "type a statement and press enter — e.g. SELECT * FROM main.sales.orders LIMIT 50",
            )
            .style(Style::default().fg(p.dim))
            .block(results_block);
            f.render_widget(par, parts[1]);
        }
        Some(Err(e)) => {
            let par = Paragraph::new(format!("✗ {e}"))
                .style(Style::default().fg(p.err))
                .wrap(Wrap { trim: false })
                .block(results_block);
            f.render_widget(par, parts[1]);
        }
        Some(Ok(data)) if data.rows.is_empty() && data.headers.is_empty() => {
            let par = Paragraph::new("✓ statement succeeded — no result set")
                .style(Style::default().fg(p.ok))
                .block(results_block);
            f.render_widget(par, parts[1]);
        }
        Some(Ok(data)) => {
            let (shown, constraints) = sliced.unwrap_or_default();
            let header_cells: Vec<Cell> = shown
                .iter()
                .map(|&i| {
                    Cell::from(data.headers[i].as_str()).style(
                        Style::default()
                            .fg(p.warehouses)
                            .add_modifier(Modifier::BOLD),
                    )
                })
                .collect();
            let header = Row::new(header_cells);
            let rows: Vec<Row> = data
                .rows
                .iter()
                .skip(console.scroll)
                .map(|r| {
                    Row::new(
                        shown
                            .iter()
                            .map(|&i| {
                                Cell::from(r.get(i).map(String::as_str).unwrap_or(""))
                                    .style(Style::default().fg(p.text))
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            let table = Table::new(rows, constraints)
                .header(header)
                .column_spacing(1)
                .block(results_block);
            f.render_widget(table, parts[1]);
            // Borders plus the header row eat three lines of the viewport.
            scrollbar(
                f,
                parts[1],
                data.rows.len(),
                parts[1].height.saturating_sub(3) as usize,
                console.scroll,
                p,
            );
        }
    }
}

/// Tab-completion popup, anchored under the segment being completed.
fn draw_sql_complete(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let (Some(console), Some(comp)) = (&app.sql, &app.sql_complete) else {
        return;
    };
    // Mirror the prompt geometry from draw_sql to find the anchor column.
    let inner_w = area.width.saturating_sub(4) as usize;
    let hscroll = (console.cursor + 3).saturating_sub(inner_w);
    let anchor = (area.x as usize + 4 + comp.seg_start.saturating_sub(hscroll)) as u16;

    let lines: Vec<Line> = if comp.loading {
        vec![Line::from(Span::styled(
            format!("{} fetching names…", app.spinner()),
            Style::default().fg(p.warn),
        ))]
    } else {
        // Keep the highlighted candidate inside the window.
        let visible = 8.min(comp.items.len());
        let skip = comp.index.saturating_sub(visible.saturating_sub(1));
        comp.items
            .iter()
            .enumerate()
            .skip(skip)
            .take(visible)
            .map(|(i, item)| {
                let style = if i == comp.index {
                    Style::default()
                        .fg(p.key)
                        .add_modifier(Modifier::REVERSED | Modifier::BOLD)
                } else {
                    Style::default().fg(p.text)
                };
                Line::from(Span::styled(format!(" {item} "), style))
            })
            .collect()
    };
    let widest = comp
        .items
        .iter()
        .map(|i| i.chars().count())
        .max()
        .unwrap_or(16);
    let width = (widest as u16 + 4).clamp(20, 44).min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(3));
    let popup = Rect {
        x: anchor.min(area.x + area.width.saturating_sub(width)),
        y: area.y + 3,
        width,
        height,
    };
    let title = if comp.loading {
        String::new()
    } else {
        format!(" {}/{} ", comp.index + 1, comp.items.len())
    };
    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(p.dim)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.key));
    f.render_widget(Clear, popup);
    f.render_widget(Paragraph::new(lines).block(block), popup);
}

/// Natural display width of each column in `cols`: the widest of header
/// and sampled cells, clamped so one huge value can't eat the pane.
fn grid_widths(data: &TableData, cols: &[usize]) -> Vec<u16> {
    cols.iter()
        .map(|&i| {
            let mut w = data.headers.get(i).map(|h| h.chars().count()).unwrap_or(0);
            for row in data.rows.iter().take(50) {
                if let Some(c) = row.get(i) {
                    w = w.max(c.chars().count());
                }
            }
            w.clamp(4, 36) as u16
        })
        .collect()
}

/// The slice of `cols` that fits in `avail` starting at offset `col`,
/// with Length constraints — at least one column so the grid never
/// vanishes. Wide results page through here instead of squeezing every
/// column into an unreadable sliver.
fn grid_slice(
    data: &TableData,
    cols: &[usize],
    col: usize,
    avail: usize,
) -> (Vec<usize>, Vec<Constraint>) {
    let widths = grid_widths(data, cols);
    let col = col.min(cols.len().saturating_sub(1));
    let mut take = 0usize;
    let mut used = 0usize;
    for w in widths.iter().skip(col) {
        let next = used + *w as usize + if take > 0 { 1 } else { 0 };
        if take > 0 && next > avail {
            break;
        }
        used = next;
        take += 1;
    }
    let shown: Vec<usize> = cols.iter().skip(col).take(take).cloned().collect();
    let constraints: Vec<Constraint> = widths
        .iter()
        .skip(col)
        .take(take)
        .map(|&w| Constraint::Length(w))
        .collect();
    (shown, constraints)
}

fn draw_preview(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let pv = app.preview.as_ref().unwrap();
    let acc = p.catalog;
    let mut title_spans: Vec<Span<'static>> = vec![
        Span::styled(" ◫ ", Style::default().fg(acc)),
        Span::styled(
            format!("{} · preview ", pv.name),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("via {} ", pv.warehouse), Style::default().fg(p.dim)),
    ];
    let make_block = |spans: Vec<Span<'static>>| {
        Block::default()
            .title(Line::from(spans))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(acc).add_modifier(Modifier::BOLD))
            .padding(Padding::horizontal(1))
    };

    match &pv.data {
        None => {
            let par = Paragraph::new(format!(
                "{} running SELECT * LIMIT 50 — the warehouse may need a moment to start…",
                app.spinner()
            ))
            .style(Style::default().fg(p.warn))
            .block(make_block(title_spans));
            f.render_widget(par, area);
        }
        Some(Err(e)) => {
            let text = format!(
                "✗ {e}\n\nwarehouse: {} ({})\nprofile: {} · host: {}\npress esc, then P to pick a different warehouse",
                pv.warehouse,
                pv.warehouse_id,
                app.profile.as_deref().unwrap_or("default"),
                app.host.as_deref().unwrap_or("unknown"),
            );
            let par = Paragraph::new(text)
                .style(Style::default().fg(p.err))
                .wrap(Wrap { trim: false })
                .block(make_block(title_spans));
            f.render_widget(par, area);
        }
        Some(Ok(data)) => {
            title_spans.push(Span::styled(
                format!("· {} rows ", data.rows.len()),
                Style::default().fg(p.dim),
            ));
            let cols = pv.visible_cols();
            if !pv.filter.is_empty() {
                title_spans.push(Span::styled(
                    format!("· cols /{} ", pv.filter),
                    Style::default().fg(p.warn),
                ));
            }
            if cols.is_empty() {
                let par = Paragraph::new(format!(
                    "no columns match /{} — backspace to widen, esc to clear",
                    pv.filter
                ))
                .style(Style::default().fg(p.warn))
                .block(make_block(title_spans));
                f.render_widget(par, area);
                return;
            }
            if pv.record {
                // Transposed: one row, fields stacked — the readable way
                // through a table with hundreds of columns.
                let row_n = pv.scroll.min(data.rows.len().saturating_sub(1));
                title_spans.push(Span::styled(
                    format!("· row {}/{} ", row_n + 1, data.rows.len()),
                    Style::default().fg(p.warn).add_modifier(Modifier::BOLD),
                ));
                let name_w = cols
                    .iter()
                    .map(|&i| data.headers[i].chars().count())
                    .max()
                    .unwrap_or(0)
                    .min(32);
                let empty: Vec<String> = Vec::new();
                let row = data.rows.get(row_n).unwrap_or(&empty);
                let lines: Vec<Line> = cols
                    .iter()
                    .map(|&i| {
                        let value = row.get(i).map(String::as_str).unwrap_or("");
                        let vstyle = if value == "␀" {
                            Style::default().fg(p.dim)
                        } else {
                            Style::default().fg(p.text)
                        };
                        Line::from(vec![
                            Span::styled(
                                format!("{:<name_w$}  ", data.headers[i]),
                                Style::default().fg(acc),
                            ),
                            Span::styled(value.to_string(), vstyle),
                        ])
                    })
                    .collect();
                let total = lines.len();
                let par = Paragraph::new(lines)
                    .scroll((pv.rscroll, 0))
                    .block(make_block(title_spans));
                f.render_widget(par, area);
                scrollbar(
                    f,
                    area,
                    total,
                    area.height.saturating_sub(2) as usize,
                    pv.rscroll as usize,
                    p,
                );
                return;
            }
            let avail = area.width.saturating_sub(4) as usize; // borders + padding
            let (shown, constraints) = grid_slice(data, &cols, pv.col, avail);
            if shown.len() < cols.len() {
                let first = pv.col.min(cols.len() - 1) + 1;
                title_spans.push(Span::styled(
                    format!(
                        "· cols {}–{} of {} ",
                        first,
                        first + shown.len() - 1,
                        cols.len()
                    ),
                    Style::default().fg(p.dim),
                ));
            }
            let header = Row::new(
                shown
                    .iter()
                    .map(|&i| {
                        Cell::from(data.headers[i].clone())
                            .style(Style::default().fg(acc).add_modifier(Modifier::BOLD))
                    })
                    .collect::<Vec<_>>(),
            );
            let rows: Vec<Row> = data
                .rows
                .iter()
                .skip(pv.scroll)
                .map(|r| {
                    Row::new(
                        shown
                            .iter()
                            .map(|&i| {
                                Cell::from(r.get(i).cloned().unwrap_or_default())
                                    .style(Style::default().fg(p.text))
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            let table = Table::new(rows, constraints)
                .header(header)
                .column_spacing(1)
                .block(make_block(title_spans));
            f.render_widget(table, area);
            // Borders plus the header row eat three lines of the viewport.
            scrollbar(
                f,
                area,
                data.rows.len(),
                area.height.saturating_sub(3) as usize,
                pv.scroll,
                p,
            );
        }
    }
}

fn draw_detail(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let d = app.detail.as_ref().unwrap();
    let acc = accent(d.panel, p);
    let title = Line::from(vec![
        Span::styled(format!(" {} ", d.panel.icon()), Style::default().fg(acc)),
        Span::styled(
            format!("{} · {} ", d.panel.title(), d.name),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
    ]);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(acc).add_modifier(Modifier::BOLD))
        .padding(Padding::horizontal(1));

    let Some(data) = &d.data else {
        let par = Paragraph::new(format!("{} Loading…", app.spinner()))
            .style(Style::default().fg(p.warn))
            .block(block);
        f.render_widget(par, area);
        return;
    };

    // Fall back to raw when there's nothing structured to show (e.g. errors).
    if d.show_raw || data.summary.is_empty() {
        let par = Paragraph::new(data.raw.as_str())
            .style(Style::default().fg(p.text))
            .wrap(Wrap { trim: false })
            .scroll((d.scroll, 0))
            .block(block);
        f.render_widget(par, area);
        let total = wrapped_height(&data.raw, area.width.saturating_sub(4) as usize);
        scrollbar(
            f,
            area,
            total,
            area.height.saturating_sub(2) as usize,
            d.scroll as usize,
            p,
        );
        return;
    }

    let mut lines: Vec<Line> = data
        .summary
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(format!("{:<16}", k), Style::default().fg(p.dim)),
                Span::styled(v.as_str(), Style::default().fg(p.text)),
            ])
        })
        .collect();
    if !data.activity.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            d.section,
            Style::default().fg(acc).add_modifier(Modifier::BOLD),
        )));
        for (status, text) in &data.activity {
            // Lineage tree guides carry their own structure — no dot.
            let treeish = text.is_empty() || text.starts_with(['├', '└', '│', '▲', '▼', ' ']);
            if treeish {
                lines.push(Line::from(Span::styled(
                    text.as_str(),
                    Style::default().fg(status_color(status, p)),
                )));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("● ", Style::default().fg(status_color(status, p))),
                    Span::styled(text.as_str(), Style::default().fg(p.text)),
                ]));
            }
        }
    }
    let total = lines.len();
    let par = Paragraph::new(lines).scroll((d.scroll, 0)).block(block);
    f.render_widget(par, area);
    scrollbar(
        f,
        area,
        total,
        area.height.saturating_sub(2) as usize,
        d.scroll as usize,
        p,
    );
}

fn draw_run(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let rv = app.run_view.as_ref().unwrap();
    let acc = accent(rv.panel, p);
    let noun = if rv.panel == Panel::Jobs {
        "run"
    } else {
        "update"
    };
    let mut title_spans = vec![
        Span::styled(format!(" {} ", rv.panel.icon()), Style::default().fg(acc)),
        Span::styled(
            format!("{} · {noun} ", rv.owner_name),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some((run_id, _, age)) = rv.runs.get(rv.idx) {
        title_spans.push(Span::styled(
            format!("{run_id} "),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ));
        title_spans.push(Span::styled(
            format!("· {} of {} · {} ", rv.idx + 1, rv.runs.len(), age),
            Style::default().fg(p.dim),
        ));
    }
    if rv.live {
        title_spans.push(Span::styled(
            format!("{} live ", app.spinner()),
            Style::default().fg(p.warn).add_modifier(Modifier::BOLD),
        ));
    }
    if rv.show_output {
        title_spans.push(Span::styled(
            if rv.live {
                "· output (tailing) "
            } else {
                "· output "
            },
            Style::default().fg(p.warn).add_modifier(Modifier::BOLD),
        ));
    }
    if rv.show_timeline {
        title_spans.push(Span::styled(
            "· timeline ",
            Style::default().fg(p.warn).add_modifier(Modifier::BOLD),
        ));
    }
    if rv.show_dag {
        title_spans.push(Span::styled(
            "· dag ",
            Style::default().fg(p.warn).add_modifier(Modifier::BOLD),
        ));
    }
    let block = Block::default()
        .title(Line::from(title_spans))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(acc).add_modifier(Modifier::BOLD))
        .padding(Padding::horizontal(1));

    // Full task output/logs, layered over the run detail.
    if rv.show_output {
        let par = match &rv.output {
            Some(text) => {
                let lines: Vec<Line> = text.lines().map(|l| output_line(l, p)).collect();
                Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .scroll((rv.scroll, 0))
                    .block(block)
            }
            None => Paragraph::new(format!(
                "{} fetching task output — one call per task…",
                app.spinner()
            ))
            .style(Style::default().fg(p.warn))
            .block(block),
        };
        f.render_widget(par, area);
        if let Some(text) = &rv.output {
            let total = wrapped_height(text, area.width.saturating_sub(4) as usize);
            scrollbar(
                f,
                area,
                total,
                area.height.saturating_sub(2) as usize,
                rv.scroll as usize,
                p,
            );
        }
        return;
    }

    let Some(data) = &rv.data else {
        let par = Paragraph::new(format!("{} Loading run…", app.spinner()))
            .style(Style::default().fg(p.warn))
            .block(block);
        f.render_widget(par, area);
        return;
    };

    if rv.show_timeline {
        draw_run_timeline(f, area, rv.scroll, data, block, p);
        return;
    }

    if rv.show_dag {
        draw_run_dag(f, area, rv.scroll, data, block, p);
        return;
    }

    if rv.show_raw || data.summary.is_empty() {
        let par = Paragraph::new(data.raw.as_str())
            .style(Style::default().fg(p.text))
            .wrap(Wrap { trim: false })
            .scroll((rv.scroll, 0))
            .block(block);
        f.render_widget(par, area);
        let total = wrapped_height(&data.raw, area.width.saturating_sub(4) as usize);
        scrollbar(
            f,
            area,
            total,
            area.height.saturating_sub(2) as usize,
            rv.scroll as usize,
            p,
        );
        return;
    }

    let mut lines: Vec<Line> = data
        .summary
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(format!("{:<16}", k), Style::default().fg(p.dim)),
                Span::styled(v.as_str(), Style::default().fg(p.text)),
            ])
        })
        .collect();
    if !data.activity.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            if rv.panel == Panel::Jobs {
                "Tasks"
            } else {
                "Event log"
            },
            Style::default().fg(acc).add_modifier(Modifier::BOLD),
        )));
        for (status, text) in &data.activity {
            // Error continuation lines carry their own ↳ marker.
            if text.starts_with("  ↳") {
                lines.push(Line::from(Span::styled(
                    text.as_str(),
                    Style::default().fg(p.err),
                )));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("● ", Style::default().fg(status_color(status, p))),
                    Span::styled(text.as_str(), Style::default().fg(p.text)),
                ]));
            }
        }
    }
    let total = lines.len();
    let par = Paragraph::new(lines).scroll((rv.scroll, 0)).block(block);
    f.render_widget(par, area);
    scrollbar(
        f,
        area,
        total,
        area.height.saturating_sub(2) as usize,
        rv.scroll as usize,
        p,
    );
}

/// Gantt chart of a job run: one bar per task on a shared time axis,
/// colored by state, so the long pole is visible at a glance.
fn draw_run_timeline(
    f: &mut Frame,
    area: Rect,
    scroll: u16,
    data: &DetailData,
    block: Block,
    p: &Palette,
) {
    let tasks = crate::fetchers::runs::timeline(&data.raw);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let t0 = tasks.iter().filter(|t| t.start > 0).map(|t| t.start).min();
    let Some(t0) = t0 else {
        let par = Paragraph::new(
            "no per-task timing recorded — legacy single-task runs carry none (J shows the raw JSON)",
        )
        .style(Style::default().fg(p.dim))
        .wrap(Wrap { trim: false })
        .block(block);
        f.render_widget(par, area);
        return;
    };
    let t1 = tasks
        .iter()
        .filter(|t| t.start > 0)
        .map(|t| t.end.unwrap_or(now).max(t.start))
        .max()
        .unwrap_or(t0);
    let span = (t1 - t0).max(1);

    let name_w = tasks
        .iter()
        .map(|t| t.key.chars().count())
        .max()
        .unwrap_or(4)
        .clamp(4, 24);
    let inner_w = area.width.saturating_sub(4) as usize;
    // Room for the widest duration ("59m 59s…" = 8) after the bar.
    let bar_w = inner_w.saturating_sub(name_w + 2 + 9).max(10);

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            format!(
                "◷ {} task{} · span {}",
                tasks.len(),
                if tasks.len() == 1 { "" } else { "s" },
                crate::shape::fmt_duration_ms(span)
            ),
            Style::default().fg(p.dim),
        )),
        Line::default(),
    ];
    for t in &tasks {
        let mut key: String = t.key.chars().take(name_w).collect();
        if key.len() < t.key.len() {
            key.pop();
            key.push('…');
        }
        let name = Span::styled(format!("{key:<name_w$}  "), Style::default().fg(p.text));
        if t.start == 0 {
            lines.push(Line::from(vec![
                name,
                Span::styled(
                    format!("– {}", t.status.label().to_lowercase()),
                    Style::default().fg(p.dim),
                ),
            ]));
            continue;
        }
        let end = t.end.unwrap_or(now).max(t.start);
        let off = ((t.start - t0) as f64 / span as f64 * bar_w as f64) as usize;
        let len = (((end - t.start) as f64 / span as f64) * bar_w as f64).round() as usize;
        let len = len.clamp(1, bar_w.saturating_sub(off).max(1));
        let dur = format!(
            " {}{}",
            crate::shape::fmt_duration_ms(end - t.start),
            if t.end.is_none() { "…" } else { "" }
        );
        lines.push(Line::from(vec![
            name,
            Span::styled(" ".repeat(off), Style::default()),
            Span::styled(
                "█".repeat(len),
                Style::default().fg(status_color(&t.status, p)),
            ),
            Span::styled(dur, Style::default().fg(p.dim)),
        ]));
    }
    let total = lines.len();
    let par = Paragraph::new(lines).scroll((scroll, 0)).block(block);
    f.render_widget(par, area);
    scrollbar(
        f,
        area,
        total,
        area.height.saturating_sub(2) as usize,
        scroll as usize,
        p,
    );
}

/// Dependency tree of a job run's tasks: each task under the task it
/// waits for, colored by state, with extra dependencies annotated.
fn draw_run_dag(
    f: &mut Frame,
    area: Rect,
    scroll: u16,
    data: &DetailData,
    block: Block,
    p: &Palette,
) {
    let rows = crate::fetchers::runs::dag(&data.raw);
    if rows.is_empty() {
        let par = Paragraph::new(
            "no task graph recorded — legacy single-task runs carry none (J shows the raw JSON)",
        )
        .style(Style::default().fg(p.dim))
        .wrap(Wrap { trim: false })
        .block(block);
        f.render_widget(par, area);
        return;
    }
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            format!(
                "⧉ {} task{} · order of execution, top to bottom",
                rows.len(),
                if rows.len() == 1 { "" } else { "s" },
            ),
            Style::default().fg(p.dim),
        )),
        Line::default(),
    ];
    for r in &rows {
        let mut spans = vec![
            Span::styled(r.prefix.clone(), Style::default().fg(p.dim)),
            Span::styled("● ", Style::default().fg(status_color(&r.status, p))),
            Span::styled(r.key.clone(), Style::default().fg(p.text)),
            Span::styled(
                format!("  {}", r.status.label().to_lowercase()),
                Style::default().fg(status_color(&r.status, p)),
            ),
        ];
        if let Some(d) = r.duration {
            spans.push(Span::styled(
                format!("  ·  {}", crate::shape::fmt_duration_ms(d)),
                Style::default().fg(p.dim),
            ));
        }
        if !r.also_after.is_empty() {
            spans.push(Span::styled(
                format!("  (also after {})", r.also_after.join(", ")),
                Style::default().fg(p.dim),
            ));
        }
        lines.push(Line::from(spans));
    }
    let total = lines.len();
    let par = Paragraph::new(lines).scroll((scroll, 0)).block(block);
    f.render_widget(par, area);
    scrollbar(
        f,
        area,
        total,
        area.height.saturating_sub(2) as usize,
        scroll as usize,
        p,
    );
}

fn draw_header(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.border));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Brand block: a two-tone brick mark, then DATABRICKS in the theme's
    // gradient with a bright shimmer sweeping across it while data loads,
    // then LAKEHOUSE letter-spaced in the accent color.
    let (from, to) = (p.grad_from, p.grad_to);
    let grad = |t: f32| {
        Color::Rgb(
            (from.0 as f32 + (to.0 as f32 - from.0 as f32) * t) as u8,
            (from.1 as f32 + (to.1 as f32 - from.1 as f32) * t) as u8,
            (from.2 as f32 + (to.2 as f32 - from.2 as f32) * t) as u8,
        )
    };
    let mut left = vec![
        Span::styled(
            " ◢",
            Style::default().fg(grad(0.0)).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "◤ ",
            Style::default().fg(grad(0.6)).add_modifier(Modifier::BOLD),
        ),
    ];
    let word: Vec<char> = "DATABRICKS".chars().collect();
    let sweep = (app.spinner_frame() * 2) % (word.len() + 8);
    for (i, ch) in word.iter().enumerate() {
        let t = i as f32 / (word.len() - 1) as f32;
        let style = if app.loading && (i as i32 - sweep as i32).unsigned_abs() <= 1 {
            Style::default().fg(p.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(grad(t)).add_modifier(Modifier::BOLD)
        };
        left.push(Span::styled(ch.to_string(), style));
    }
    left.push(Span::raw(" "));
    for ch in "LAKEHOUSE".chars() {
        left.push(Span::styled(format!("{ch} "), Style::default().fg(p.key)));
    }
    left.push(Span::styled(
        format!("· v{}", env!("CARGO_PKG_VERSION")),
        Style::default().fg(p.dim),
    ));
    if let Some(profile) = &app.profile {
        left.push(Span::styled("  ·  ", Style::default().fg(p.dim)));
        left.push(Span::styled("⌂ ", Style::default().fg(p.dim)));
        left.push(Span::styled(profile.as_str(), Style::default().fg(p.key)));
    }
    if let Some(Shape::Badge(b)) = &app.user_badge {
        left.push(Span::styled("  ·  ", Style::default().fg(p.dim)));
        left.push(Span::styled(
            format!("{} {}", b.label, b.value),
            Style::default().fg(p.key),
        ));
    }
    if app.zoomed {
        left.push(Span::styled("  ·  ", Style::default().fg(p.dim)));
        left.push(Span::styled(
            format!("⛶ {}", app.focus.title()),
            Style::default()
                .fg(accent(app.focus, p))
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Workspace health at a glance: counts of items per status bucket.
    let (ok, pending, failed, idle) = app.status_counts();
    for (count, glyph, color) in [
        (ok, "●", p.ok),
        (pending, "◐", p.warn),
        (failed, "✗", p.err),
        (idle, "○", p.dim),
    ] {
        if count > 0 {
            left.push(Span::styled("  ", Style::default()));
            left.push(Span::styled(
                format!("{glyph} {count}"),
                Style::default().fg(color),
            ));
        }
    }
    f.render_widget(Paragraph::new(Line::from(left)), inner);

    let right = if app.loading {
        Line::from(Span::styled(
            format!("{} refreshing ", app.spinner()),
            Style::default().fg(p.warn),
        ))
    } else {
        Line::from(Span::styled(
            format!("updated {}s ago ", app.last_refresh_age().as_secs()),
            Style::default().fg(p.dim),
        ))
    };
    f.render_widget(Paragraph::new(right).alignment(Alignment::Right), inner);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let key =
        |k: &'static str| Span::styled(k, Style::default().fg(p.key).add_modifier(Modifier::BOLD));
    let dim = |t: &'static str| Span::styled(t, Style::default().fg(p.dim));

    if let Some(confirm) = &app.confirm {
        let line = Line::from(vec![
            Span::styled(
                format!(" ⚠ {} ", confirm.message),
                Style::default().fg(p.warn).add_modifier(Modifier::BOLD),
            ),
            key("y"),
            dim(" confirm · any other key cancels"),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }

    if app.filter_entry {
        let line = Line::from(vec![
            Span::styled(
                format!(" /{}", app.active_filter()),
                Style::default().fg(p.warn).add_modifier(Modifier::BOLD),
            ),
            Span::styled("▏", Style::default().fg(p.warn)),
            dim("  type to filter   "),
            key("enter"),
            dim(" apply   "),
            key("esc"),
            dim(" clear"),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }

    if let Some(pv) = &app.preview {
        if pv.filter_entry {
            let line = Line::from(vec![
                Span::styled(
                    format!(" cols /{}", pv.filter),
                    Style::default().fg(p.warn).add_modifier(Modifier::BOLD),
                ),
                Span::styled("▏", Style::default().fg(p.warn)),
                dim("  type to filter columns   "),
                key("enter"),
                dim(" apply   "),
                key("esc"),
                dim(" clear"),
            ]);
            f.render_widget(Paragraph::new(line), area);
            return;
        }
    }

    let mut spans = if app.secret_form.is_some() {
        vec![
            dim(" type   "),
            key("enter"),
            dim(" next / save   "),
            key("esc"),
            dim(" cancel"),
        ]
    } else if app.help {
        vec![
            dim(" "),
            key("j"),
            dim("/"),
            key("k"),
            dim(" scroll   any other key closes"),
        ]
    } else if app.pane_cfg.is_some() {
        vec![
            dim(" "),
            key("j"),
            dim("/"),
            key("k"),
            dim(" select   "),
            key("space"),
            dim(" show/hide   "),
            key("J"),
            dim("/"),
            key("K"),
            dim(" move   "),
            key("esc"),
            dim(" done"),
        ]
    } else if app.jump.is_some() {
        vec![
            dim(" type to search everything   "),
            key("\u{2191}"),
            dim("/"),
            key("\u{2193}"),
            dim(" select   "),
            key("enter"),
            dim(" jump   "),
            key("esc"),
            dim(" close"),
        ]
    } else if app.problems.is_some() {
        vec![
            dim(" "),
            key("j"),
            dim("/"),
            key("k"),
            dim(" select   "),
            key("enter"),
            dim(" jump to item   "),
            key("esc"),
            dim(" close"),
        ]
    } else if app.upcoming.is_some() {
        vec![
            dim(" "),
            key("j"),
            dim("/"),
            key("k"),
            dim(" select   "),
            key("enter"),
            dim(" jump to job   "),
            key("esc"),
            dim(" close"),
        ]
    } else if app.cost.is_some() {
        vec![
            dim(" "),
            key("esc"),
            dim(" back   "),
            key("q"),
            dim(" quit"),
        ]
    } else if app.wh_picker.is_some() {
        vec![
            dim(" "),
            key("j"),
            dim("/"),
            key("k"),
            dim(" select   "),
            key("enter"),
            dim(" run preview   "),
            key("esc"),
            dim(" cancel"),
        ]
    } else if app.sql.is_some() {
        if app.hist_search.is_some() {
            vec![
                dim(" type to search history   "),
                key("^r"),
                dim(" older match   "),
                key("enter"),
                dim(" accept   "),
                key("esc"),
                dim(" cancel"),
            ]
        } else {
            vec![
                dim(" "),
                key("enter"),
                dim(" run   "),
                key("tab"),
                dim(" complete   "),
                key("↑"),
                dim("/"),
                key("↓"),
                dim(" history   "),
                key("^r"),
                dim(" search   "),
                key("^x"),
                dim(" editor   "),
                key("pgup"),
                dim("/"),
                key("pgdn"),
                dim(" scroll   "),
                key("⇧←→"),
                dim(" cols   "),
                key("^s"),
                dim(" export   "),
                key("esc"),
                dim(if app.sql.as_ref().is_some_and(|c| c.running) {
                    " cancel query"
                } else {
                    " close"
                }),
            ]
        }
    } else if let Some(pv) = &app.preview {
        if pv.record {
            vec![
                dim(" "),
                key("j"),
                dim("/"),
                key("k"),
                dim(" fields   "),
                key("h"),
                dim("/"),
                key("l"),
                dim(" prev/next row   "),
                key("v"),
                dim(" grid   "),
                key("/"),
                dim(" filter cols   "),
                key("esc"),
                dim(" back"),
            ]
        } else {
            vec![
                dim(" "),
                key("esc"),
                dim(" back   "),
                key("j"),
                dim("/"),
                key("k"),
                dim(" rows   "),
                key("←"),
                dim("/"),
                key("→"),
                dim(" columns   "),
                key("v"),
                dim(" row view   "),
                key("/"),
                dim(" filter cols   "),
                key("e"),
                dim(" export csv   "),
                key("q"),
                dim(" quit"),
            ]
        }
    } else if app.picker.is_some() {
        vec![
            dim(" "),
            key("j"),
            dim("/"),
            key("k"),
            dim(" select   "),
            key("enter"),
            dim(" switch workspace   "),
            key("esc"),
            dim(" cancel"),
        ]
    } else if let Some(rv) = &app.run_view {
        if rv.show_output {
            vec![
                dim(" "),
                key("esc"),
                dim(" back to run   "),
                key("j"),
                dim("/"),
                key("k"),
                dim(" scroll   "),
                key("q"),
                dim(" quit"),
            ]
        } else {
            let mut spans = vec![
                dim(" "),
                key("esc"),
                dim(" back   "),
                key("h"),
                dim("/"),
                key("l"),
                dim(" older/newer   "),
            ];
            if rv.panel == Panel::Jobs {
                spans.push(key("o"));
                spans.push(dim(" output   "));
                spans.push(key("t"));
                spans.push(dim(" timeline   "));
                spans.push(key("d"));
                spans.push(dim(" dag   "));
                spans.push(key("r"));
                spans.push(dim(" repair   "));
            }
            spans.extend([
                key("s"),
                dim(" cancel   "),
                key("j"),
                dim("/"),
                key("k"),
                dim(" scroll   "),
                key("J"),
                dim(" raw   "),
                key("q"),
                dim(" quit"),
            ]);
            spans
        }
    } else if app.detail.is_some() {
        let mut spans = vec![dim(" "), key("esc"), dim(" back   ")];
        match app.detail.as_ref() {
            Some(d) if d.panel == Panel::Jobs && d.section != "Lineage" => {
                spans.push(key("enter"));
                spans.push(dim(" latest run   "));
            }
            Some(d) if d.panel == Panel::Pipelines && d.section != "Lineage" => {
                spans.push(key("enter"));
                spans.push(dim(" latest update   "));
            }
            _ => {}
        }
        spans.extend([
            key("j"),
            dim("/"),
            key("k"),
            dim(" scroll   "),
            key("J"),
            dim(" raw   "),
            key("o"),
            dim(" open   "),
            key("q"),
            dim(" quit"),
        ]);
        spans
    } else {
        let mut spans = vec![
            dim(" "),
            key("tab"),
            dim("/"),
            key("h"),
            dim("/"),
            key("l"),
            dim(" switch   "),
            key("j"),
            dim("/"),
            key("k"),
            dim(" select   "),
            key("/"),
            dim(if app.active_filter().is_empty() {
                " filter   "
            } else {
                " filter · esc clears   "
            }),
        ];
        // Enter means different things per pane — and nothing on a secret key.
        let enter_label = match app.focus {
            Panel::Secrets if app.secret_scope.is_some() => None,
            Panel::Secrets => Some(" open scope   "),
            Panel::Catalog if app.uc_path.len() < 2 => Some(" open   "),
            _ => Some(" details   "),
        };
        if let Some(label) = enter_label {
            spans.push(key("enter"));
            spans.push(dim(label));
        }
        // Only show keys that do something in the focused pane.
        match app.focus {
            Panel::Catalog => {
                if !app.uc_path.is_empty() {
                    spans.push(key("bksp"));
                    spans.push(dim(" up   "));
                }
                if app.uc_path.len() == 2 {
                    spans.push(key("p"));
                    spans.push(dim(" preview   "));
                    spans.push(key("L"));
                    spans.push(dim(" lineage   "));
                    spans.push(key(":"));
                    spans.push(dim(" query table   "));
                }
            }
            Panel::Dashboards => {}
            Panel::Secrets => {
                if app.secret_scope.is_some() {
                    spans.push(key("bksp"));
                    spans.push(dim(" up   "));
                }
                spans.push(key("a"));
                spans.push(dim(if app.secret_scope.is_some() {
                    " add secret   "
                } else {
                    " new scope   "
                }));
                spans.push(key("x"));
                spans.push(dim(" delete   "));
            }
            _ => {
                spans.push(key("s"));
                spans.push(dim(" action   "));
            }
        }
        spans.extend([
            key("g"),
            dim(" access   "),
            key("$"),
            dim(" cost   "),
            key("!"),
            dim(" problems   "),
            key("u"),
            dim(" upcoming   "),
        ]);
        // At the catalog's table level the ':' hint already reads "query table".
        if !(app.focus == Panel::Catalog && app.uc_path.len() == 2) {
            spans.push(key(":"));
            spans.push(dim(" sql   "));
        }
        spans.extend([
            key("o"),
            dim(" open   "),
            key("z"),
            dim(if app.zoomed { " unzoom   " } else { " zoom   " }),
            key("w"),
            dim(" workspace   "),
            key("t"),
            dim(" theme   "),
            key("r"),
            dim(" refresh   "),
            key("?"),
            dim(" help   "),
            key("q"),
            dim(" quit"),
        ]);
        spans
    };
    // A flash message prefixes the key hints instead of hiding them.
    if let Some((msg, _)) = &app.flash {
        let color = if msg.starts_with('✗') { p.err } else { p.ok };
        let mut prefixed = vec![
            Span::styled(
                format!(" {msg}"),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            dim("  ·  "),
        ];
        prefixed.append(&mut spans);
        spans = prefixed;
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[allow(clippy::too_many_arguments)]
fn draw_panel(
    f: &mut Frame,
    area: Rect,
    panel: Panel,
    shape: Option<&Shape>,
    focused: bool,
    selected: Option<usize>,
    fresh: bool,
    breadcrumb: &str,
    spinner: &str,
    filter: &str,
    entering: bool,
    p: &Palette,
) {
    let accent = accent(panel, p);
    // Unfocused panes keep their status colors (the cross-pane signal)
    // but their text steps back so the active pane pops.
    let dimmed = |s: Style| {
        if focused {
            s
        } else {
            s.add_modifier(Modifier::DIM)
        }
    };
    let (border_style, title_style) = if focused {
        (
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        )
    } else {
        (Style::default().fg(p.border), Style::default().fg(p.dim))
    };
    let visible: Option<Vec<&crate::shape::ListItem>> = match shape {
        Some(Shape::List(items)) => Some(
            items
                .iter()
                .filter(|it| crate::shape::item_matches(it, filter))
                .collect(),
        ),
        _ => None,
    };
    let count = match (&visible, shape) {
        // With a filter active, show how many made the cut.
        (Some(v), Some(Shape::List(items))) if !filter.is_empty() || entering => {
            format!(" · {}/{}", v.len(), items.len())
        }
        (Some(v), _) => format!(" · {}", v.len()),
        (_, Some(Shape::Table(data))) => format!(" · {}", data.rows.len()),
        _ => String::new(),
    };
    let crumb = if !breadcrumb.is_empty() {
        format!(" ▸ {breadcrumb}")
    } else {
        String::new()
    };
    // A short reversed flash on the title when fresh data lands.
    let title_style = if fresh {
        Style::default()
            .bg(accent)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        title_style
    };
    let mut title_spans = vec![
        Span::styled(format!(" {} ", panel.icon()), Style::default().fg(accent)),
        Span::styled(format!("{}{}{} ", panel.title(), crumb, count), title_style),
    ];
    if entering || !filter.is_empty() {
        title_spans.push(Span::styled(
            format!("/{}{} ", filter, if entering { "▏" } else { "" }),
            Style::default().fg(p.warn).add_modifier(Modifier::BOLD),
        ));
    }
    let title = Line::from(title_spans);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(if focused {
            BorderType::Thick
        } else {
            BorderType::Rounded
        })
        .border_style(border_style)
        .padding(Padding::horizontal(1));

    match shape {
        None => {
            let par = Paragraph::new(format!("{} syncing with the lakehouse…", spinner))
                .style(Style::default().fg(p.warn))
                .block(block);
            f.render_widget(par, area);
        }
        Some(Shape::List(items)) if visible.as_ref().is_none_or(|v| v.is_empty()) => {
            let empty = if !items.is_empty() {
                format!("∅ nothing matches /{filter}")
            } else {
                let msg = match panel {
                    Panel::Clusters => "no compute running",
                    Panel::Jobs => "no jobs defined",
                    Panel::Pipelines => "no pipelines",
                    Panel::Warehouses => "no warehouses",
                    Panel::Dashboards => "no dashboards yet",
                    Panel::Catalog => "nothing at this level",
                    Panel::Secrets => "no secret scopes",
                };
                format!("∅ {msg}")
            };
            let par = Paragraph::new(empty)
                .style(Style::default().fg(p.dim))
                .block(block);
            f.render_widget(par, area);
        }
        Some(Shape::List(_)) => {
            let list_items: Vec<ListItem> = visible
                .unwrap_or_default()
                .iter()
                .map(|item| {
                    let color = status_color(&item.status, p);
                    let chip = match item.status {
                        Status::Stopped | Status::Unknown(_) => Span::styled(
                            format!(" {} ", item.status.label()),
                            Style::default().fg(color).add_modifier(Modifier::DIM),
                        ),
                        _ => Span::styled(
                            format!(" {} ", item.status.label()),
                            Style::default()
                                .bg(color)
                                .fg(Color::Black)
                                .add_modifier(Modifier::BOLD),
                        ),
                    };
                    let mut spans = vec![
                        Span::styled("● ", Style::default().fg(color)),
                        Span::styled(item.name.as_str(), dimmed(Style::default().fg(p.text))),
                        Span::raw("  "),
                        chip,
                    ];
                    if !item.history.is_empty() {
                        spans.push(Span::raw("  "));
                        for run in &item.history {
                            spans.push(Span::styled(
                                history_glyph(run),
                                Style::default().fg(status_color(run, p)),
                            ));
                        }
                    }
                    if let Some(detail) = &item.detail {
                        spans.push(Span::styled(
                            format!("  {}", detail),
                            dimmed(Style::default().fg(p.dim)),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();
            let list = List::new(list_items)
                .block(block)
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
            match selected {
                Some(sel) => {
                    let mut state = ListState::default().with_selected(Some(sel));
                    f.render_stateful_widget(list, area, &mut state);
                }
                None => f.render_widget(list, area),
            }
        }
        Some(Shape::Table(data)) => {
            let header_cells: Vec<Cell> = data
                .headers
                .iter()
                .map(|h| {
                    Cell::from(h.as_str()).style(Style::default().add_modifier(Modifier::BOLD))
                })
                .collect();
            let header = Row::new(header_cells).style(Style::default().fg(accent));
            let rows: Vec<Row> = data
                .rows
                .iter()
                .map(|r| {
                    Row::new(
                        r.iter()
                            .map(|c| Cell::from(c.as_str()).style(dimmed(Style::default())))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            let widths: Vec<Constraint> = data
                .headers
                .iter()
                .map(|_| Constraint::Percentage(100 / data.headers.len() as u16))
                .collect();
            let table = Table::new(rows, widths).header(header).block(block);
            f.render_widget(table, area);
        }
        Some(Shape::Badge(b)) => {
            let text = format!("{}: {}", b.label, b.value);
            let par = Paragraph::new(text)
                .style(dimmed(Style::default().fg(p.text)))
                .block(block);
            f.render_widget(par, area);
        }
        Some(Shape::Text(t)) => {
            let color = if t.starts_with('✗') { p.err } else { p.text };
            let par = Paragraph::new(t.as_str())
                .style(dimmed(Style::default().fg(color)))
                .wrap(Wrap { trim: false })
                .block(block);
            f.render_widget(par, area);
        }
    }
}

/// Vertical scrollbar on the right border of a bordered block when the
/// content overflows the viewport; `total` and `pos` are in rendered
/// lines (or rows).
fn scrollbar(f: &mut Frame, area: Rect, total: usize, viewport: usize, pos: usize, p: &Palette) {
    if viewport == 0 || total <= viewport {
        return;
    }
    let max = total - viewport;
    let mut state = ScrollbarState::new(max).position(pos.min(max));
    let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .track_style(Style::default().fg(p.border))
        .thumb_symbol("┃")
        .thumb_style(Style::default().fg(p.text));
    f.render_stateful_widget(
        sb,
        area.inner(Margin {
            horizontal: 0,
            vertical: 1,
        }),
        &mut state,
    );
}

/// Rendered height of text wrapped at `width` columns.
fn wrapped_height(text: &str, width: usize) -> usize {
    let w = width.max(1);
    text.lines()
        .map(|l| l.chars().count().div_ceil(w).max(1))
        .sum()
}

/// One rendered line of task output: section headers stand out, error
/// and warning lines are tinted, leading timestamps are dimmed so the
/// message carries the color.
fn output_line<'a>(line: &'a str, p: &Palette) -> Line<'a> {
    if let Some(rest) = line.strip_prefix("── ") {
        let color = if rest.contains("FAILED") {
            p.err
        } else if rest.contains("RUNNING") || rest.contains("PENDING") {
            p.warn
        } else if rest.contains("SUCCESS") {
            p.ok
        } else {
            p.text
        };
        return Line::from(Span::styled(
            line,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    }
    let trimmed = line.trim_start();
    let indented = line.starts_with(char::is_whitespace);
    let body = if line.contains("ERROR")
        || line.contains("FATAL")
        || line.contains("Exception")
        || trimmed.starts_with("Traceback")
        || trimmed.starts_with("Caused by")
        || (indented && (trimmed.starts_with("at ") || trimmed.starts_with("File \"")))
    {
        Style::default().fg(p.err)
    } else if line.contains("WARN") {
        Style::default().fg(p.warn)
    } else if line == "logs (tail):" || trimmed.starts_with("(no output") {
        Style::default().fg(p.dim)
    } else {
        Style::default().fg(p.text)
    };
    let ts = timestamp_prefix(line);
    if ts > 0 {
        Line::from(vec![
            Span::styled(&line[..ts], Style::default().fg(p.dim)),
            Span::styled(&line[ts..], body),
        ])
    } else {
        Line::from(Span::styled(line, body))
    }
}

/// Byte length of a leading timestamp-ish prefix ("26/07/18 12:34:56 ",
/// ISO dates), 0 when there is none. Requires enough digits that plain
/// leading numbers don't get dimmed.
fn timestamp_prefix(line: &str) -> usize {
    let mut len = 0;
    let mut digits = 0;
    for (i, c) in line.char_indices() {
        if c.is_ascii_digit() {
            digits += 1;
        } else if !matches!(c, '/' | '-' | ':' | '.' | ',' | 'T' | 'Z' | ' ') {
            break;
        }
        len = i + c.len_utf8();
        if len >= 30 {
            break;
        }
    }
    if digits >= 6 && len >= 8 {
        len
    } else {
        0
    }
}

fn status_color(status: &Status, p: &Palette) -> Color {
    match status {
        Status::Running | Status::Success => p.ok,
        Status::Stopped => p.dim,
        Status::Pending => p.warn,
        Status::Failed => p.err,
        Status::Unknown(_) => p.text,
    }
}

fn history_glyph(status: &Status) -> &'static str {
    match status {
        Status::Success => "✓",
        Status::Failed => "✗",
        Status::Running => "●",
        Status::Pending => "◐",
        Status::Stopped => "○",
        Status::Unknown(_) => "·",
    }
}
