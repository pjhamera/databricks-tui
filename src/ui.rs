use crate::app::{App, Panel, ThemeMode};
use crate::shape::{Shape, Status};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Padding, Paragraph,
        Row, Table, Wrap,
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
            &app.uc_path.join("."),
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
        return;
    }

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[1]);

    let rows = [
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ];
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints(rows)
        .split(body[0]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints(rows)
        .split(body[1]);

    let areas = [left[0], left[1], right[0], right[1], right[2], left[2]];

    for (i, panel) in Panel::ALL.iter().enumerate() {
        let focused = app.focus == *panel;
        let shape = app.shapes[i].as_ref();
        let selected = focused.then(|| app.selection(i));
        let fresh = app.updated_at[i]
            .map(|t| t.elapsed() < Duration::from_millis(1200))
            .unwrap_or(false);
        draw_panel(
            f,
            areas[i],
            *panel,
            shape,
            focused,
            selected,
            fresh,
            &app.uc_path.join("."),
            app.spinner(),
            &app.filters[i],
            app.filter_entry && focused,
            &p,
        );
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
                        format!("{:>8.1} DBU · ${:.2}", s.dbus, s.usd)
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
    let results_block = Block::default()
        .title(Line::from(vec![
            Span::styled(" Results ", Style::default().fg(p.text)),
            Span::styled(row_info, Style::default().fg(p.dim)),
        ]))
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
            let header_cells: Vec<Cell> = data
                .headers
                .iter()
                .map(|h| {
                    Cell::from(h.as_str()).style(
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
                        r.iter()
                            .map(|c| Cell::from(c.as_str()).style(Style::default().fg(p.text)))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            let n = data.headers.len().max(1) as u16;
            let widths: Vec<Constraint> = data
                .headers
                .iter()
                .map(|_| Constraint::Ratio(1, n as u32))
                .collect();
            let table = Table::new(rows, widths)
                .header(header)
                .column_spacing(1)
                .block(results_block);
            f.render_widget(table, parts[1]);
        }
    }
}

fn draw_preview(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let pv = app.preview.as_ref().unwrap();
    let acc = p.catalog;
    let row_info = match &pv.data {
        Some(Ok(t)) => format!(" · {} rows", t.rows.len()),
        _ => String::new(),
    };
    let title = Line::from(vec![
        Span::styled(" ◫ ", Style::default().fg(acc)),
        Span::styled(
            format!("{}{} · preview ", pv.name, row_info),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("via {} ", pv.warehouse), Style::default().fg(p.dim)),
    ]);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(acc).add_modifier(Modifier::BOLD))
        .padding(Padding::horizontal(1));

    match &pv.data {
        None => {
            let par = Paragraph::new(format!(
                "{} running SELECT * LIMIT 50 — the warehouse may need a moment to start…",
                app.spinner()
            ))
            .style(Style::default().fg(p.warn))
            .block(block);
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
                .block(block);
            f.render_widget(par, area);
        }
        Some(Ok(data)) => {
            let header_cells: Vec<Cell> = data
                .headers
                .iter()
                .map(|h| {
                    Cell::from(h.as_str())
                        .style(Style::default().fg(acc).add_modifier(Modifier::BOLD))
                })
                .collect();
            let header = Row::new(header_cells);
            let rows: Vec<Row> = data
                .rows
                .iter()
                .skip(pv.scroll)
                .map(|r| {
                    Row::new(
                        r.iter()
                            .map(|c| Cell::from(c.as_str()).style(Style::default().fg(p.text)))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            let n = data.headers.len().max(1) as u16;
            let widths: Vec<Constraint> = data
                .headers
                .iter()
                .map(|_| Constraint::Ratio(1, n as u32))
                .collect();
            let table = Table::new(rows, widths)
                .header(header)
                .column_spacing(1)
                .block(block);
            f.render_widget(table, area);
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
            lines.push(Line::from(vec![
                Span::styled("● ", Style::default().fg(status_color(status, p))),
                Span::styled(text.as_str(), Style::default().fg(p.text)),
            ]));
        }
    }
    let par = Paragraph::new(lines).scroll((d.scroll, 0)).block(block);
    f.render_widget(par, area);
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
    let block = Block::default()
        .title(Line::from(title_spans))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(acc).add_modifier(Modifier::BOLD))
        .padding(Padding::horizontal(1));

    let Some(data) = &rv.data else {
        let par = Paragraph::new(format!("{} Loading run…", app.spinner()))
            .style(Style::default().fg(p.warn))
            .block(block);
        f.render_widget(par, area);
        return;
    };

    if rv.show_raw || data.summary.is_empty() {
        let par = Paragraph::new(data.raw.as_str())
            .style(Style::default().fg(p.text))
            .wrap(Wrap { trim: false })
            .scroll((rv.scroll, 0))
            .block(block);
        f.render_widget(par, area);
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
    let par = Paragraph::new(lines).scroll((rv.scroll, 0)).block(block);
    f.render_widget(par, area);
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

    if let Some((msg, _)) = &app.flash {
        let color = if msg.starts_with('✗') { p.err } else { p.ok };
        let line = Line::from(Span::styled(format!(" {msg}"), Style::default().fg(color)));
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

    let spans = if app.problems.is_some() {
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
                key("^s"),
                dim(" export   "),
                key("esc"),
                dim(" close"),
            ]
        }
    } else if app.preview.is_some() {
        vec![
            dim(" "),
            key("esc"),
            dim(" back   "),
            key("j"),
            dim("/"),
            key("k"),
            dim(" scroll rows   "),
            key("e"),
            dim(" export csv   "),
            key("q"),
            dim(" quit"),
        ]
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
    } else if app.run_view.is_some() {
        vec![
            dim(" "),
            key("esc"),
            dim(" back   "),
            key("h"),
            dim("/"),
            key("l"),
            dim(" older/newer   "),
            key("j"),
            dim("/"),
            key("k"),
            dim(" scroll   "),
            key("J"),
            dim(" raw   "),
            key("q"),
            dim(" quit"),
        ]
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
            key("enter"),
            dim(if app.focus == Panel::Catalog && app.uc_path.len() < 2 {
                " open   "
            } else {
                " details   "
            }),
        ];
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
            key("q"),
            dim(" quit"),
        ]);
        spans
    };
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
    let crumb = if panel == Panel::Catalog && !breadcrumb.is_empty() {
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
                        Span::styled(item.name.as_str(), Style::default().fg(p.text)),
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
                            Style::default().fg(p.dim),
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
                .map(|r| Row::new(r.iter().map(|c| Cell::from(c.as_str())).collect::<Vec<_>>()))
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
                .style(Style::default().fg(p.text))
                .block(block);
            f.render_widget(par, area);
        }
        Some(Shape::Text(t)) => {
            let color = if t.starts_with('✗') { p.err } else { p.text };
            let par = Paragraph::new(t.as_str())
                .style(Style::default().fg(color))
                .wrap(Wrap { trim: false })
                .block(block);
            f.render_widget(par, area);
        }
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
