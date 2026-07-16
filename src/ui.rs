use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Clear},
};

use crate::app::{App, AtmosOverlay, Confirm, EditorTool};
use crate::material::Material;
use crate::world::World;

/// Terminal rows reserved for the status bar. World height is sized against this
/// fixed reserve so ground cells stay visible even when the bar wraps.
pub const MAX_STATUS_ROWS: u16 = 3;

/// Draw the whole frame: filled-cell material grid + a status line + optional
/// material-picker overlay.
pub fn draw(frame: &mut Frame, world: &World, app: &App) {
    let area = frame.area();
    // Always reserve the max status height so the sim ground matches the canvas.
    let grid_area = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(MAX_STATUS_ROWS),
    );
    draw_grid(frame, world, app, &grid_area);
    draw_overlay_legend(frame, app.atmos_overlay, &grid_area);
    let sr = status_rows(app, area.width);
    draw_status(frame, app, &area, sr);
    if app.brush_options_open {
        draw_brush_options(frame, &area, app);
    }
    if app.picker.tool_picker_open {
        draw_tool_picker(frame, &area, app);
    }
    if app.picker.picker_open {
        draw_picker(frame, &area, app);
    }
    if app.scene_menu.open {
        draw_scene_menu(frame, &area, app);
    }
    // Confirmation dialog drawn last (on top).
    if app.status.confirm != Confirm::None {
        draw_confirmation(frame, &area, app);
    }
}

fn overlay_value_color(overlay: AtmosOverlay, value: i32) -> Option<Color> {
    let scale = |value: i32, max: i32| (value.clamp(0, max) * 255 / max) as u8;
    match overlay {
        AtmosOverlay::None => None,
        AtmosOverlay::Pressure => {
            if value <= 256 {
                let v = scale(value, 256);
                Some(Color::Rgb(v / 3, v / 2, v))
            } else {
                let v = scale(value - 256, 768);
                Some(Color::Rgb(128 + v / 2, 80u8.saturating_sub(v / 4), 32))
            }
        }
        AtmosOverlay::Oxygen => {
            let v = scale(value, 26);
            Some(Color::Rgb(0, v, v))
        }
        AtmosOverlay::Fuel => {
            let v = scale(value, 64);
            Some(Color::Rgb(v, v / 2, 0))
        }
        AtmosOverlay::Exhaust => {
            let v = scale(value, 64);
            Some(Color::Rgb(v / 2, v / 3, v))
        }
        AtmosOverlay::Temperature => {
            if value <= 20 {
                let v = scale(value + 200, 220);
                Some(Color::Rgb(0, v / 2, v))
            } else {
                let v = scale(value - 20, 980);
                Some(Color::Rgb(v, 64u8.saturating_sub(v / 4), 0))
            }
        }
    }
}

fn overlay_color(world: &World, overlay: AtmosOverlay, x: usize, y: usize) -> Option<Color> {
    let value = match overlay {
        AtmosOverlay::None => return None,
        AtmosOverlay::Pressure => world.pressure_at(x, y),
        AtmosOverlay::Oxygen => world.atmosphere_at(x, y).1 as i32,
        AtmosOverlay::Fuel => world.atmosphere_at(x, y).2 as i32,
        AtmosOverlay::Exhaust => world.atmosphere_at(x, y).3 as i32,
        AtmosOverlay::Temperature => world.temp_at(x, y) as i32,
    };
    overlay_value_color(overlay, value)
}

fn draw_overlay_legend(frame: &mut Frame, overlay: AtmosOverlay, area: &Rect) {
    let (title, min, max, labels) = match overlay {
        AtmosOverlay::None => return,
        AtmosOverlay::Pressure => ("Pressure", 0, 1_024, "0   1   4 atm"),
        AtmosOverlay::Oxygen => ("Oxygen", 0, 26, "0  13  26 units"),
        AtmosOverlay::Fuel => ("Fuel vapor", 0, 64, "0  32  64 units"),
        AtmosOverlay::Exhaust => ("Exhaust", 0, 64, "0  32  64 units"),
        AtmosOverlay::Temperature => ("Temperature", -200, 1_000, "-200  20  1000 C"),
    };
    if area.width < 12 || area.height < 2 {
        return;
    }

    let text = format!(" {title}: {labels} ");
    let width = (text.chars().count() as u16).min(area.width);
    let x = area.x + area.width - width;
    let bg = Color::Rgb(16, 18, 26);
    let fg = Color::Rgb(230, 232, 240);
    let buf = frame.buffer_mut();
    for offset in 0..width {
        if let Some(cell) = buf.cell_mut((x + offset, area.y)) {
            cell.set_char(text.chars().nth(offset as usize).unwrap_or(' '));
            cell.set_fg(fg);
            cell.set_bg(bg);
        }
        let value = if width <= 1 {
            min
        } else {
            min + (max - min) * offset as i32 / (width - 1) as i32
        };
        if let Some(cell) = buf.cell_mut((x + offset, area.y + 1)) {
            cell.set_char(' ');
            cell.set_bg(overlay_value_color(overlay, value).unwrap_or(bg));
        }
    }
}

fn draw_grid(frame: &mut Frame, world: &World, app: &App, area: &Rect) {
    let buf = frame.buffer_mut();
    let grid_rows = area.height as usize;
    let tick = world.tick();
    let preview_color = app.preview_color(tick);
    let line_preview = app.line_preview_cells();
    let shape_preview_contains = |x, y| {
        line_preview.as_ref().map_or_else(
            || app.preview_contains(x, y),
            |cells| cells.contains(&(x, y)),
        )
    };
    for cy in 0..grid_rows {
        for cx in 0..area.width as usize {
            let (wx, top_y, bottom_y) = if app.viewport.zoom == 2 {
                (
                    app.viewport.camera.0 + (cx / 2) as i32,
                    app.viewport.camera.1 + cy as i32,
                    None,
                )
            } else {
                (
                    app.viewport.camera.0 + cx as i32,
                    app.viewport.camera.1 + (cy * 2) as i32,
                    Some(app.viewport.camera.1 + (cy * 2 + 1) as i32),
                )
            };
            let cell = buf.cell_mut((area.x + cx as u16, area.y + cy as u16));
            let Some(cell) = cell else {
                continue;
            };
            let skip =
                wx < 0 || top_y < 0 || wx as usize >= world.width || top_y as usize >= world.height;
            let skip = skip || bottom_y.is_some_and(|by| by < 0 || by as usize >= world.height);
            if skip {
                continue;
            }
            let ghost_top = app.paste_ghost_at(wx, top_y, world.width, world.height);
            let top_color = ghost_top
                .map(|(material, life, seed, _)| material.color(seed, life, tick))
                .unwrap_or_else(|| {
                    overlay_color(world, app.atmos_overlay, wx as usize, top_y as usize)
                        .unwrap_or_else(|| {
                            let (material, seed, life) =
                                world.render_state(wx as usize, top_y as usize);
                            material.color(seed, life, tick)
                        })
                });
            if let Some(bottom_y) = bottom_y {
                let ghost_bottom = app.paste_ghost_at(wx, bottom_y, world.width, world.height);
                let bottom_color = ghost_bottom
                    .map(|(material, life, seed, _)| material.color(seed, life, tick))
                    .unwrap_or_else(|| {
                        overlay_color(world, app.atmos_overlay, wx as usize, bottom_y as usize)
                            .unwrap_or_else(|| {
                                let (material, seed, life) =
                                    world.render_state(wx as usize, bottom_y as usize);
                                material.color(seed, life, tick)
                            })
                    });
                cell.set_char('▀');
                cell.set_fg(top_color);
                cell.set_bg(bottom_color);
            } else {
                cell.set_char(' ');
                cell.set_bg(top_color);
            }
            let brush_preview = app.brush_preview_contains(wx, top_y, world.width, world.height);
            let shape_preview = app.tool != EditorTool::Select && shape_preview_contains(wx, top_y);
            let selected = brush_preview || shape_preview;
            let selected_bottom = bottom_y.is_some_and(|y| {
                app.brush_preview_contains(wx, y, world.width, world.height)
                    || (app.tool != EditorTool::Select && shape_preview_contains(wx, y))
            });
            if selected {
                if app.viewport.zoom == 2 {
                    cell.set_bg(preview_color);
                } else {
                    cell.set_fg(preview_color);
                }
            }
            if selected_bottom {
                cell.set_bg(preview_color);
            }

            let selection_top = app.selection_contains(wx, top_y)
                || (app.tool == EditorTool::Select && shape_preview_contains(wx, top_y));
            let selection_bottom = bottom_y.is_some_and(|y| {
                app.selection_contains(wx, y)
                    || (app.tool == EditorTool::Select && shape_preview_contains(wx, y))
            });
            if selection_top {
                if app.viewport.zoom == 2 {
                    cell.set_bg(Color::White);
                } else {
                    cell.set_fg(Color::White);
                }
            }
            if selection_bottom {
                cell.set_bg(Color::White);
            }
        }
    }
}

/// Build the unified status text (without rendering it).
fn status_text(app: &App) -> String {
    let erase = if app.brush_erase { " · Erase" } else { "" };
    let mirror = app
        .mirror
        .map(|axis| format!(" · {}", axis.name()))
        .unwrap_or_default();
    let dirty = if app.dirty { " ●" } else { "" };
    let paused = if app.paused { "Paused" } else { "Running" };
    let overlay = format!(" · Overlay {}", app.atmos_overlay.name());
    let coords = app
        .mouse_world
        .map(|(x, y)| format!(" · {x},{y}"))
        .unwrap_or_default();
    let controls = if app.pasting {
        "Click Paste · Esc Cancel"
    } else if app.selection.is_some() {
        "Ctrl+C Copy · Ctrl+X Cut · Del Delete · Esc Brush"
    } else {
        "Tab Materials · E Tools · B Brush · S Scenes · A Air · O Overlay · Space Pause · Wheel Zoom · Esc Quit"
    };
    format!(
        "  ▀ {} · {} {} r{}{}{} · {}{} · {}{}{}  │  {}",
        app.selected.name(),
        app.tool.name(),
        app.brush_shape.name(),
        app.brush,
        erase,
        mirror,
        app.scene_name,
        dirty,
        paused,
        overlay,
        coords,
        controls,
    )
}

/// How many terminal rows the status bar needs (capped at `MAX_STATUS_ROWS`).
pub(crate) fn status_rows(app: &App, terminal_width: u16) -> u16 {
    let w = terminal_width.max(1) as usize;
    let chars = status_text(app).chars().count();
    let needed = chars.div_ceil(w);
    (needed as u16).clamp(1, MAX_STATUS_ROWS)
}

fn draw_status(frame: &mut Frame, app: &App, area: &Rect, status_rows: u16) {
    let buf = frame.buffer_mut();
    let bg = Color::Rgb(16, 18, 26);
    let fg = Color::Rgb(210, 214, 224);
    let accent = Color::Rgb(255, 220, 120);
    let width = area.width as usize;

    // Always clear the full reserved status block so wrapped/unwrapped status
    // never leaves stale grid cells visible above the taskbar.
    let reserve_top = area.y + area.height.saturating_sub(MAX_STATUS_ROWS);
    for y in reserve_top..area.y + area.height {
        for x in 0..area.width {
            if let Some(cell) = buf.cell_mut((area.x + x, y)) {
                cell.set_char(' ');
                cell.set_bg(bg);
            }
        }
    }

    // Pin status text to the bottom of the reserved block.
    let base_y = area.y + area.height - status_rows;

    // Status message takes priority if active.
    if let Some(msg) = &app.status.status_msg {
        let msg_color = if msg.starts_with("Error: ") {
            Color::Rgb(255, 120, 100)
        } else {
            Color::Rgb(120, 255, 160)
        };
        let sy = base_y; // message always fits one row
        for (i, ch) in msg.chars().enumerate() {
            let x = area.x + i as u16;
            if x >= area.x + area.width {
                break;
            }
            if let Some(cell) = buf.cell_mut((x, sy)) {
                cell.set_char(ch);
                cell.set_fg(msg_color);
                cell.set_bg(bg);
            }
        }
        return;
    }

    let s = status_text(app);

    // print wrapped text across status_rows
    for (i, ch) in s.chars().enumerate() {
        let row = i / width;
        if row >= status_rows as usize {
            break;
        }
        let x = area.x + (i % width) as u16;
        let y = base_y + row as u16;
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_char(ch);
            cell.set_fg(fg);
            cell.set_bg(bg);
        }
    }

    // tint the swatch ▀ (col 2) with the selected material's colour, and the
    // name with the accent colour so the active material pops.
    let swatch_col = app.selected.color(0, 128, 0);
    let sy = base_y; // first status row
    if let Some(cell) = buf.cell_mut((area.x + 2, sy)) {
        cell.set_fg(swatch_col);
    }
    let name = app.selected.name();
    for (k, _) in name.chars().enumerate() {
        if let Some(cell) = buf.cell_mut((area.x + 4 + k as u16, sy)) {
            cell.set_fg(accent);
        }
    }
}

pub fn brush_options_rect(w: u16, h: u16) -> Rect {
    let width = 46.min(w.saturating_sub(2));
    let height = 5.min(h);
    Rect::new(
        w.saturating_sub(width) / 2,
        h.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn draw_brush_options(frame: &mut Frame, area: &Rect, app: &App) {
    let popup = brush_options_rect(area.width, area.height);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Brush Options — ←→ change, B/Esc close ")
            .style(
                Style::default()
                    .fg(Color::Rgb(210, 214, 224))
                    .bg(Color::Rgb(18, 20, 30)),
            ),
        popup,
    );

    let rows = [
        ("Shape", app.brush_shape.name().to_string()),
        (
            "Radius",
            format!(
                "{} ({}×{})",
                app.brush,
                app.brush * 2 + 1,
                app.brush * 2 + 1
            ),
        ),
        (
            "Erase",
            if app.brush_erase { "On" } else { "Off" }.to_string(),
        ),
    ];
    let buf = frame.buffer_mut();
    let base_bg = Color::Rgb(18, 20, 30);
    let hi_bg = Color::Rgb(44, 48, 66);
    let accent = Color::Rgb(255, 220, 120);
    let inner_w = popup.width.saturating_sub(2);

    for (index, (label, value)) in rows.iter().enumerate() {
        let y = popup.y + 1 + index as u16;
        if y >= popup.y + popup.height.saturating_sub(1) {
            break;
        }
        let selected = index == app.brush_options_cursor;
        let bg = if selected { hi_bg } else { base_bg };
        for dx in 0..inner_w {
            if let Some(cell) = buf.cell_mut((popup.x + 1 + dx, y)) {
                cell.set_bg(bg);
            }
        }
        putc(
            buf,
            popup.x + 1,
            y,
            if selected { '▶' } else { ' ' },
            accent,
            bg,
        );
        for (offset, ch) in format!("{label:<8} {value}").chars().enumerate() {
            if offset as u16 >= inner_w.saturating_sub(2) {
                break;
            }
            putc(
                buf,
                popup.x + 3 + offset as u16,
                y,
                ch,
                Color::Rgb(210, 214, 224),
                bg,
            );
        }
    }
}

pub fn tool_picker_rect(w: u16, h: u16) -> Rect {
    let width = 26.min(w.saturating_sub(2));
    let height = (EditorTool::ALL.len() as u16 + 2).min(h.saturating_sub(2));
    Rect::new(
        w.saturating_sub(width) / 2,
        h.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn draw_tool_picker(frame: &mut Frame, area: &Rect, app: &App) {
    let popup = tool_picker_rect(area.width, area.height);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Tools — Enter to pick, Esc to close ")
            .style(
                Style::default()
                    .fg(Color::Rgb(210, 214, 224))
                    .bg(Color::Rgb(18, 20, 30)),
            ),
        popup,
    );

    let buf = frame.buffer_mut();
    let base_bg = Color::Rgb(18, 20, 30);
    let hi_bg = Color::Rgb(44, 48, 66);
    let accent = Color::Rgb(255, 220, 120);
    for (index, tool) in EditorTool::ALL.iter().enumerate() {
        let y = popup.y + 1 + index as u16;
        let selected = index == app.picker.tool_picker_cursor;
        for x in popup.x + 1..popup.x + popup.width.saturating_sub(1) {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_bg(if selected { hi_bg } else { base_bg });
            }
        }
        putc(
            buf,
            popup.x + 1,
            y,
            if selected { '▶' } else { ' ' },
            accent,
            if selected { hi_bg } else { base_bg },
        );
        for (offset, ch) in tool.name().chars().enumerate() {
            putc(
                buf,
                popup.x + 3 + offset as u16,
                y,
                ch,
                Color::Rgb(210, 214, 224),
                if selected { hi_bg } else { base_bg },
            );
        }
    }
}

/// Centred rect for the picker popup, clamped to the terminal.
pub fn picker_rect(w: u16, h: u16) -> Rect {
    let pw: u16 = 28;
    let ph: u16 = (Material::ALL.len() as u16) + 2; // items + border rows
    let pw = pw.min(w.saturating_sub(2));
    let ph = ph.min(h.saturating_sub(2));
    let x = w.saturating_sub(pw) / 2;
    let y = h.saturating_sub(ph) / 2;
    Rect::new(x, y, pw, ph)
}

fn list_offset(cursor: usize, total: usize, visible: usize) -> usize {
    if visible == 0 || total <= visible {
        0
    } else {
        cursor
            .saturating_add(1)
            .saturating_sub(visible)
            .min(total - visible)
    }
}

pub(crate) fn picker_scroll_offset(cursor: usize, popup_height: u16) -> usize {
    list_offset(
        cursor,
        Material::ALL.len(),
        popup_height.saturating_sub(2) as usize,
    )
}

fn draw_picker(frame: &mut Frame, area: &Rect, app: &App) {
    let popup = picker_rect(area.width, area.height);

    // wipe anything behind, then draw the framed panel — do this before the
    // mutable buffer borrow below.
    frame.render_widget(Clear, popup);
    let title = if app.picker.picker_query.is_empty() {
        " Materials — type to find, Enter pick, Esc close ".to_string()
    } else {
        format!(
            " Materials — \"{}\" · Enter pick, Esc close ",
            app.picker.picker_query
        )
    };
    let block = Block::default().borders(Borders::ALL).title(title).style(
        Style::default()
            .fg(Color::Rgb(210, 214, 224))
            .bg(Color::Rgb(18, 20, 30)),
    );
    frame.render_widget(block, popup);

    let buf = frame.buffer_mut();
    let accent = Color::Rgb(255, 220, 120);
    let bright = Color::Rgb(255, 236, 190);
    let dim = Color::Rgb(150, 156, 172);
    let base_bg = Color::Rgb(18, 20, 30);
    let hi_bg = Color::Rgb(44, 48, 66);

    let inner_x = popup.x + 1;
    let inner_w = popup.width.saturating_sub(2);

    let visible = popup.height.saturating_sub(2) as usize;
    let offset = picker_scroll_offset(app.picker.picker_cursor, popup.height);
    for (row, (i, &m)) in Material::ALL
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible)
        .enumerate()
    {
        let y = popup.y + 1 + row as u16;
        let selected = i == app.picker.picker_cursor;

        // row background
        for dx in 0..inner_w {
            if let Some(cell) = buf.cell_mut((inner_x + dx, y)) {
                cell.set_bg(if selected { hi_bg } else { base_bg });
            }
        }

        // cursor marker
        putc(
            buf,
            inner_x,
            y,
            if selected { '▶' } else { ' ' },
            accent,
            base_bg,
        );

        // colour swatch
        putc(buf, inner_x + 2, y, '▀', m.color(0, 128, 0), base_bg);

        // name
        for (k, ch) in m.name().chars().enumerate() {
            putc(
                buf,
                inner_x + 4 + k as u16,
                y,
                ch,
                if selected {
                    bright
                } else {
                    Color::Rgb(210, 214, 224)
                },
                base_bg,
            );
        }

        // hotkey (if any) on the right edge
        if let Some((key, _)) = Material::PALETTE.iter().find(|(_, mm)| *mm == m) {
            let label = format!("[{key}]");
            let base = inner_x + inner_w.saturating_sub(label.len() as u16);
            for (k, ch) in label.chars().enumerate() {
                putc(buf, base + k as u16, y, ch, dim, base_bg);
            }
        }
    }
}

/// Safely write a single glyph with foreground + background.
fn putc(buf: &mut Buffer, x: u16, y: u16, ch: char, fg: Color, bg: Color) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        cell.set_char(ch);
        cell.set_fg(fg);
        cell.set_bg(bg);
    }
}

/// Centred rect for the scene popup, clamped to the terminal.
pub fn scene_menu_rect(w: u16, h: u16) -> Rect {
    let pw: u16 = 44;
    let ph: u16 = 14;
    let pw = pw.min(w.saturating_sub(2));
    let ph = ph.min(h.saturating_sub(2));
    let x = w.saturating_sub(pw) / 2;
    let y = h.saturating_sub(ph) / 2;
    Rect::new(x, y, pw, ph)
}

pub(crate) fn scene_scroll_offset(cursor: usize, total: usize, popup_height: u16) -> usize {
    list_offset(cursor, total, popup_height.saturating_sub(4) as usize)
}

/// Draw the scene management popup menu.
fn draw_scene_menu(frame: &mut Frame, area: &Rect, app: &App) {
    let popup = scene_menu_rect(area.width, area.height);

    // Clear behind and draw border
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Scenes ")
        .style(
            Style::default()
                .fg(Color::Rgb(210, 214, 224))
                .bg(Color::Rgb(18, 20, 30)),
        );
    frame.render_widget(block, popup);

    let buf = frame.buffer_mut();
    let accent = Color::Rgb(255, 220, 120);
    let bright = Color::Rgb(255, 236, 190);
    let dim = Color::Rgb(150, 156, 172);
    let base_bg = Color::Rgb(18, 20, 30);
    let hi_bg = Color::Rgb(44, 48, 66);

    let inner_x = popup.x + 1;
    let inner_w = popup.width.saturating_sub(2);

    let scenes = &app.scene_menu.scenes;
    let cursor = app.scene_menu.cursor;
    let visible = popup.height.saturating_sub(4) as usize;
    let offset = scene_scroll_offset(cursor, scenes.len(), popup.height);

    if scenes.is_empty() {
        let msg = "No saved scenes";
        let msg_x = inner_x + (inner_w.saturating_sub(msg.len() as u16)) / 2;
        for (k, ch) in msg.chars().enumerate() {
            putc(buf, msg_x + k as u16, popup.y + 3, ch, dim, base_bg);
        }
    } else {
        for (row, (idx, scene_name)) in scenes
            .iter()
            .enumerate()
            .skip(offset)
            .take(visible)
            .enumerate()
        {
            let y = popup.y + 1 + row as u16;
            let selected = idx == cursor;

            for dx in 0..inner_w {
                if let Some(cell) = buf.cell_mut((inner_x + dx, y)) {
                    cell.set_bg(if selected { hi_bg } else { base_bg });
                }
            }

            putc(
                buf,
                inner_x,
                y,
                if selected { '▶' } else { ' ' },
                accent,
                if selected { hi_bg } else { base_bg },
            );

            for (k, ch) in scene_name.chars().enumerate() {
                if k as u16 >= inner_w.saturating_sub(2) {
                    break;
                }
                putc(
                    buf,
                    inner_x + 2 + k as u16,
                    y,
                    ch,
                    if selected {
                        bright
                    } else {
                        Color::Rgb(210, 214, 224)
                    },
                    if selected { hi_bg } else { base_bg },
                );
            }
        }
    }

    let hint_y = popup.y + popup.height.saturating_sub(3);
    for (line, hints) in [
        "↑↓ Navigate  Enter Load  A New  R Save copy",
        "D Delete  Shift+S Overwrite  Esc Close",
    ]
    .iter()
    .enumerate()
    {
        for (k, ch) in hints.chars().enumerate() {
            if k as u16 >= inner_w {
                break;
            }
            putc(
                buf,
                inner_x + k as u16,
                hint_y + line as u16,
                ch,
                dim,
                base_bg,
            );
        }
    }

    // Save dialog overlay (drawn on top of the menu)
    if app.scene_menu.saving {
        draw_save_dialog(buf, popup, app);
    }
}

/// Draw the save-name input dialog, centered inside the scene menu popup.
fn draw_save_dialog(buf: &mut Buffer, popup: Rect, app: &App) {
    let dw: u16 = 28;
    let dh: u16 = 4;
    let dx = popup.x + (popup.width.saturating_sub(dw)) / 2;
    let dy = popup.y + 3;
    let dialog = Rect::new(dx, dy, dw, dh);

    // Dim background behind dialog
    for y in dialog.y..dialog.y + dialog.height {
        for x in dialog.x..dialog.x + dialog.width {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_bg(Color::Rgb(8, 10, 16));
            }
        }
    }

    // Border
    for x in dialog.x..dialog.x + dialog.width {
        putc(
            buf,
            x,
            dialog.y,
            '─',
            Color::Rgb(255, 220, 120),
            Color::Rgb(8, 10, 16),
        );
        putc(
            buf,
            x,
            dialog.y + dialog.height - 1,
            '─',
            Color::Rgb(255, 220, 120),
            Color::Rgb(8, 10, 16),
        );
    }
    for y in dialog.y..dialog.y + dialog.height {
        putc(
            buf,
            dialog.x,
            y,
            '│',
            Color::Rgb(255, 220, 120),
            Color::Rgb(8, 10, 16),
        );
        putc(
            buf,
            dialog.x + dialog.width - 1,
            y,
            '│',
            Color::Rgb(255, 220, 120),
            Color::Rgb(8, 10, 16),
        );
    }
    putc(
        buf,
        dialog.x,
        dialog.y,
        '┌',
        Color::Rgb(255, 220, 120),
        Color::Rgb(8, 10, 16),
    );
    putc(
        buf,
        dialog.x + dialog.width - 1,
        dialog.y,
        '┐',
        Color::Rgb(255, 220, 120),
        Color::Rgb(8, 10, 16),
    );
    putc(
        buf,
        dialog.x,
        dialog.y + dialog.height - 1,
        '└',
        Color::Rgb(255, 220, 120),
        Color::Rgb(8, 10, 16),
    );
    putc(
        buf,
        dialog.x + dialog.width - 1,
        dialog.y + dialog.height - 1,
        '┘',
        Color::Rgb(255, 220, 120),
        Color::Rgb(8, 10, 16),
    );

    // Prompt
    let prompt = "Name:";
    for (k, ch) in prompt.chars().enumerate() {
        putc(
            buf,
            dialog.x + 2 + k as u16,
            dialog.y + 1,
            ch,
            Color::Rgb(210, 214, 224),
            Color::Rgb(8, 10, 16),
        );
    }

    // Input field
    let input_x = dialog.x + 8;
    let input = &app.scene_menu.save_name;
    for (k, ch) in input.chars().enumerate() {
        putc(
            buf,
            input_x + k as u16,
            dialog.y + 1,
            ch,
            Color::Rgb(255, 236, 190),
            Color::Rgb(8, 10, 16),
        );
    }

    // Input cursor.
    let cursor_x = input_x + input.chars().count() as u16;
    if cursor_x < dialog.x + dialog.width - 2 {
        putc(
            buf,
            cursor_x,
            dialog.y + 1,
            '▏',
            Color::Rgb(255, 220, 120),
            Color::Rgb(8, 10, 16),
        );
    }

    // Hint
    let hint = "Enter=Save  Esc=Cancel";
    let hint_x = dialog.x + (dialog.width.saturating_sub(hint.len() as u16)) / 2;
    for (k, ch) in hint.chars().enumerate() {
        putc(
            buf,
            hint_x + k as u16,
            dialog.y + 2,
            ch,
            Color::Rgb(150, 156, 172),
            Color::Rgb(8, 10, 16),
        );
    }
}

/// Draw the confirmation dialog over the centre of the terminal.
fn draw_confirmation(frame: &mut Frame, area: &Rect, app: &App) {
    let text = app.status.confirm.prompt();
    let w = text.len() as u16 + 4;
    let h: u16 = 3;
    let w = w.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    frame.render_widget(Clear, popup);
    let block = Block::default().borders(Borders::ALL).style(
        Style::default()
            .fg(Color::Rgb(255, 220, 120))
            .bg(Color::Rgb(18, 20, 30)),
    );
    frame.render_widget(block, popup);

    let buf = frame.buffer_mut();
    let inner_x = popup.x + 2;
    for (i, ch) in text.chars().enumerate() {
        let cx = inner_x + i as u16;
        if cx >= popup.x + popup.width - 2 {
            break;
        }
        putc(
            buf,
            cx,
            popup.y + 1,
            ch,
            Color::Rgb(255, 236, 190),
            Color::Rgb(18, 20, 30),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_offset_keeps_cursor_visible() {
        assert_eq!(list_offset(0, 31, 10), 0);
        assert_eq!(list_offset(9, 31, 10), 0);
        assert_eq!(list_offset(10, 31, 10), 1);
        assert_eq!(list_offset(30, 31, 10), 21);
    }

    #[test]
    fn short_lists_do_not_scroll() {
        assert_eq!(list_offset(4, 5, 10), 0);
        assert_eq!(scene_scroll_offset(4, 5, 14), 0);
    }
}
