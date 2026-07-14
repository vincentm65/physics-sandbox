use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Clear},
};

use crate::app::{App, EditorTool};
use crate::material::Material;
use crate::world::World;

/// Draw the whole frame: filled-cell material grid + a status line + optional
/// material-picker overlay.
pub fn draw(frame: &mut Frame, world: &World, app: &App) {
    let area = frame.area();
    draw_grid(frame, world, app, &area);
    draw_status(frame, app, &area);
    if app.tool_picker_open {
        draw_tool_picker(frame, &area, app);
    }
    if app.picker_open {
        draw_picker(frame, &area, app);
    }
    if app.scene_menu.open {
        draw_scene_menu(frame, &area, app);
    }
}

fn draw_grid(frame: &mut Frame, world: &World, app: &App, area: &Rect) {
    let buf = frame.buffer_mut();
    let grid_rows = (area.height as usize).saturating_sub(1);
    let tick = world.tick();
    for cy in 0..grid_rows {
        for cx in 0..area.width as usize {
            let (wx, top_y, bottom_y) = if app.zoom == 2 {
                (
                    app.camera.0 + (cx / 2) as i32,
                    app.camera.1 + cy as i32,
                    None,
                )
            } else {
                (
                    app.camera.0 + cx as i32,
                    app.camera.1 + (cy * 2) as i32,
                    Some(app.camera.1 + (cy * 2 + 1) as i32),
                )
            };
            let cell = buf.cell_mut((area.x + cx as u16, area.y + cy as u16));
            let Some(cell) = cell else {
                continue;
            };
            let skip = wx < 0 || top_y < 0 || wx as usize >= world.width || top_y as usize >= world.height;
            let skip = skip || bottom_y.is_some_and(|by| by < 0 || by as usize >= world.height);
            if skip {
                continue;
            }
            let ghost_top = app.paste_ghost_at(wx, top_y, world.width, world.height);
            let top_color = ghost_top
                .map(|(material, life, seed, _)| material.color(seed, life, tick))
                .unwrap_or_else(|| {
                    let top = world.get(wx as usize, top_y as usize);
                    top.color(
                        world.seed_at(wx as usize, top_y as usize),
                        world.life_at(wx as usize, top_y as usize),
                        tick,
                    )
                });
            if let Some(bottom_y) = bottom_y {
                let ghost_bottom = app.paste_ghost_at(wx, bottom_y, world.width, world.height);
                let bottom_color = ghost_bottom
                    .map(|(material, life, seed, _)| material.color(seed, life, tick))
                    .unwrap_or_else(|| {
                        let bottom = world.get(wx as usize, bottom_y as usize);
                        bottom.color(
                            world.seed_at(wx as usize, bottom_y as usize),
                            world.life_at(wx as usize, bottom_y as usize),
                            tick,
                        )
                    });
                cell.set_char('▀');
                cell.set_fg(top_color);
                cell.set_bg(bottom_color);
            } else {
                cell.set_char(' ');
                cell.set_bg(top_color);
            }
            let selected = app.preview_contains(wx, top_y) || app.selection_contains(wx, top_y);
            let selected_bottom = bottom_y
                .is_some_and(|y| app.preview_contains(wx, y) || app.selection_contains(wx, y));
            if selected {
                if app.zoom == 2 {
                    cell.set_bg(Color::Rgb(255, 255, 255));
                } else {
                    cell.set_fg(Color::Rgb(255, 255, 255));
                }
            }
            if selected_bottom {
                cell.set_bg(Color::Rgb(255, 255, 255));
            }
        }
    }
}

fn draw_status(frame: &mut Frame, app: &App, area: &Rect) {
    let buf = frame.buffer_mut();
    let sy = area.y + area.height - 1;
    let bg = Color::Rgb(16, 18, 26);
    let fg = Color::Rgb(210, 214, 224);
    let accent = Color::Rgb(255, 220, 120);

    // fill the row's background
    for x in 0..area.width {
        if let Some(cell) = buf.cell_mut((area.x + x, sy)) {
            cell.set_char(' ');
            cell.set_bg(bg);
        }
    }
    let name = app.selected.name();
    let tool = app.tool.name();
    let mirror = app.mirror.map(|axis| axis.name()).unwrap_or_default();
    let paused = if app.paused { " ‖" } else { "" };
    let paste = if app.pasting { " PASTE" } else { "" };
    let scene = app.scene.name();
    let s = format!(
        "  ▀ {name}   {tool}   Brush:{}/8   {mirror}   {scene}{paused}{paste}   Z=Zoom  Pan=drag  H/V=Mirror  E=Tools  S=Scenes",
        app.brush,
    );

    for (i, ch) in s.chars().enumerate() {
        let x = area.x + i as u16;
        if x >= area.x + area.width {
            break;
        }
        if let Some(cell) = buf.cell_mut((x, sy)) {
            cell.set_char(ch);
            cell.set_fg(fg);
            cell.set_bg(bg);
        }
    }

    // tint the swatch ▀ (col 2) with the selected material's colour, and the
    // name with the accent colour so the active material pops.
    let swatch_col = app.selected.color(0, 128, 0);
    if let Some(cell) = buf.cell_mut((area.x + 2, sy)) {
        cell.set_fg(swatch_col);
    }
    for (k, _) in name.chars().enumerate() {
        if let Some(cell) = buf.cell_mut((area.x + 4 + k as u16, sy)) {
            cell.set_fg(accent);
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
        let selected = index == app.tool_picker_cursor;
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

fn draw_picker(frame: &mut Frame, area: &Rect, app: &App) {
    let popup = picker_rect(area.width, area.height);

    // wipe anything behind, then draw the framed panel — do this before the
    // mutable buffer borrow below.
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Materials — Tab/Enter to pick, Esc to close ")
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

    for (i, &m) in Material::ALL.iter().enumerate() {
        let y = popup.y + 1 + i as u16;
        let selected = i == app.picker_cursor;

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
    let pw: u16 = 36;
    let ph: u16 = 14;
    let pw = pw.min(w.saturating_sub(2));
    let ph = ph.min(h.saturating_sub(2));
    let x = w.saturating_sub(pw) / 2;
    let y = h.saturating_sub(ph) / 2;
    Rect::new(x, y, pw, ph)
}

/// Draw the scene management popup menu.
fn draw_scene_menu(frame: &mut Frame, area: &Rect, app: &App) {
    let popup = scene_menu_rect(area.width, area.height);

    // Clear behind and draw border
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Scenes — ↑↓ nav  L=Load  D=Delete  A=Add/Rename  S=Save  Esc=Close ")
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

    // Scene list area: rows 1..=8 (8 items max visible)
    let list_start = 1;
    let list_end = 8;
    let scenes = &app.scene_menu.scenes;
    let cursor = app.scene_menu.cursor;

    if scenes.is_empty() {
        let msg = "No scenes yet (A=Add)";
        let msg_x = inner_x + (inner_w.saturating_sub(msg.len() as u16)) / 2;
        for (k, ch) in msg.chars().enumerate() {
            let x = msg_x + k as u16;
            putc(
                buf,
                x,
                popup.y + 4,
                ch,
                if k < 9 { accent } else { dim },
                base_bg,
            );
        }
    } else {
        for i in 0..(list_end - list_start + 1) {
            let idx: usize = i;
            if idx >= scenes.len() {
                break;
            }
            let y = popup.y + list_start as u16 + i as u16;
            let selected = idx == cursor;
            let scene_name = &scenes[idx];

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

            // scene name
            let name_x = inner_x + 2;
            for (k, ch) in scene_name.chars().enumerate() {
                putc(
                    buf,
                    name_x + k as u16,
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
        }
    }

    // Action hints at the bottom
    let hint_y = popup.y + list_end as u16 + 1;
    let hints = " L=Load  D=Delete  A=Add  R=Rename  S=Save(overwrite)  ";
    for (k, ch) in hints.chars().enumerate() {
        let x = inner_x + k as u16;
        if x >= inner_x + inner_w {
            break;
        }
        putc(buf, x, hint_y, ch, dim, base_bg);
    }

    // Save dialog overlay (drawn on top of the menu)
    if app.scene_menu.saving {
        draw_save_dialog(buf, popup, app);
    }
}

/// Draw the save-name input dialog, centered inside the scene menu popup.
fn draw_save_dialog(buf: &mut Buffer, popup: Rect, app: &App) {
    let dw: u16 = 28;
    let dh: u16 = 3;
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

    // Cursor block (blinking — always shown as solid in save mode)
    let cursor_x = input_x + input.len() as u16;
    if cursor_x < dialog.x + dialog.width - 2 {
        putc(
            buf,
            cursor_x,
            dialog.y + 1,
            ' ',
            Color::Rgb(255, 236, 190),
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
