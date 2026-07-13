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
            catalog: Color::Rgb(255, 140, 66),
        },
        // Light theme uses explicit darker shades that stay readable on a white background.
        ThemeMode::Light => Palette {
            text: Color::Black,
            dim: Color::Rgb(107, 114, 128),
            border: Color::Rgb(156, 163, 175),
            warn: Color::Rgb(180, 83, 9),
            ok: Color::Rgb(21, 128, 61),
            err: Color::Rgb(185, 28, 28),
            key: Color::Rgb(8, 145, 178),
            brand: Color::Rgb(220, 38, 38),
            clusters: Color::Rgb(8, 145, 178),
            jobs: Color::Rgb(162, 28, 175),
            pipelines: Color::Rgb(21, 128, 61),
            warehouses: Color::Rgb(29, 78, 216),
            catalog: Color::Rgb(194, 65, 12),
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

    if app.cost.is_some() {
        draw_cost(f, root[1], app, &p);
        return;
    }

    if app.preview.is_some() {
        draw_preview(f, root[1], app, &p);
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
                "Run preview on ",
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
    let title = Line::from(vec![
        Span::styled(" ◢◤ ", Style::default().fg(p.brand)),
        Span::styled(
            "Usage · last 14 days ",
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

            let par = Paragraph::new(lines).block(block);
            f.render_widget(par, area);
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

fn draw_header(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.border));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Brand block: a two-tone brick mark, then DATABRICKS in a red→orange
    // gradient with a bright shimmer sweeping across it while data loads,
    // then LAKEHOUSE letter-spaced in the accent color.
    let (from, to) = match app.theme {
        ThemeMode::Dark => ((255u8, 54u8, 33u8), (255u8, 160u8, 70u8)),
        ThemeMode::Light => ((185u8, 28u8, 28u8), (194u8, 65u8, 12u8)),
    };
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

    let spans = if app.cost.is_some() {
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
    } else if app.preview.is_some() {
        vec![
            dim(" "),
            key("esc"),
            dim(" back   "),
            key("j"),
            dim("/"),
            key("k"),
            dim(" scroll rows   "),
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
    } else if app.detail.is_some() {
        vec![
            dim(" "),
            key("esc"),
            dim(" back   "),
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
        ]
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
