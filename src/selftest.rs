//! Headless, deterministic checks for the simulation. Run with `--selftest`.
//! Lets us verify every material interaction without a terminal attached.

use crate::app::{App, BrushShape, Confirm, EditorTool};
use crate::material::Material;
use crate::world::AMBIENT_AIR_MASS;
use crate::world::AMBIENT_O2;
use crate::world::{FUSE_BURN_TICKS, Scene, World};
use Material::*;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

fn count(w: &World, m: Material) -> usize {
    (0..w.height)
        .flat_map(|y| (0..w.width).map(move |x| w.get(x, y)))
        .filter(|&c| c == m)
        .count()
}

fn min_y(w: &World, m: Material) -> Option<usize> {
    (0..w.height)
        .filter(|&y| (0..w.width).any(|x| w.get(x, y) == m))
        .min()
}

fn floor(w: &mut World) {
    for x in 0..w.width {
        w.paint(x, w.height - 1, Stone);
    }
}

type Test = fn() -> Result<(), String>;

fn sand_falls() -> Result<(), String> {
    let mut w = World::new(12, 12);
    floor(&mut w);
    w.paint(5, 1, Sand);
    for _ in 0..40 {
        w.step();
    }
    let y = min_y(&w, Sand).ok_or("sand vanished")?;
    if y < 9 {
        return Err(format!("sand did not fall (ended at y={y})"));
    }
    Ok(())
}

fn fast_fall_is_bounded_and_cannot_tunnel() -> Result<(), String> {
    let mut open = World::new(7, 8);
    open.paint(3, 1, Sand);
    open.step();
    if open.get(3, 1) != Sand {
        return Err("sand moved before fractional acceleration accumulated".into());
    }
    open.step();
    open.step();
    if open.get(3, 2) != Sand {
        return Err("sand did not move after fractional acceleration accumulated".into());
    }

    // Metal is a fixed solid (stone would fall as an unsupported structural chunk).
    let mut blocked = World::new(7, 8);
    for x in 0..7 {
        blocked.paint(x, 2, Metal);
    }
    blocked.paint(3, 1, Sand);
    for _ in 0..3 {
        blocked.step();
    }
    if blocked.get(3, 1) != Sand || blocked.get(3, 2) != Metal {
        return Err("fast fall passed through a one-cell barrier".into());
    }
    Ok(())
}

fn water_prefers_route_to_lower_space() -> Result<(), String> {
    let mut w = World::new(11, 7);
    // Fixed platform: unsupported stone would collapse before water routes.
    for x in 0..11 {
        if x != 8 {
            w.paint(x, 4, Metal);
        }
    }
    w.paint(5, 3, Water);
    for _ in 0..3 {
        w.step();
    }
    // Hydrostatic surface preference should send the cell toward the nearby
    // hole; unit lateral velocity may already have dropped it through.
    let reached_outlet = (0..w.height).any(|y| w.get(8, y) == Water);
    if !reached_outlet {
        return Err("water did not choose the nearby downward outlet".into());
    }
    Ok(())
}

fn wall_is_immovable() -> Result<(), String> {
    let mut w = World::new(8, 8);
    // Metal stays put; unsupported stone is a falling structural chunk.
    w.paint(3, 3, Metal);
    for _ in 0..20 {
        w.step();
    }
    if w.get(3, 3) != Metal {
        return Err("wall moved".into());
    }
    Ok(())
}

fn water_spreads() -> Result<(), String> {
    let mut w = World::new(24, 12);
    floor(&mut w);
    for y in 1..5 {
        w.paint(12, y, Water);
    }
    let initial = count(&w, Water);
    for _ in 0..300 {
        w.step();
    }
    let on_floor = (0..w.width)
        .filter(|&x| w.get(x, w.height - 2) == Water)
        .count();
    if count(&w, Water) != initial {
        return Err("water was not conserved".into());
    }
    if on_floor < 3 {
        return Err(format!("water did not spread (floor cells={on_floor})"));
    }
    Ok(())
}

fn sand_sinks_in_water() -> Result<(), String> {
    let mut w = World::new(12, 12);
    for y in 0..12 {
        w.paint(0, y, Stone);
        w.paint(11, y, Stone);
    }
    floor(&mut w);
    for y in 7..11 {
        for x in 1..11 {
            w.paint(x, y, Water);
        }
    }
    w.paint(5, 1, Sand);
    for _ in 0..300 {
        w.step();
    }
    let y = min_y(&w, Sand).ok_or("sand vanished")?;
    if y < 8 {
        return Err(format!("sand did not sink (ended at y={y})"));
    }
    Ok(())
}

fn oil_floats() -> Result<(), String> {
    let mut w = World::new(12, 12);
    for y in 0..12 {
        w.paint(0, y, Stone);
        w.paint(11, y, Stone);
    }
    floor(&mut w);
    // oil at the bottom, water above it -> oil should rise.
    for y in 9..11 {
        for x in 1..11 {
            w.paint(x, y, Oil);
        }
    }
    for y in 6..9 {
        for x in 1..11 {
            w.paint(x, y, Water);
        }
    }
    for _ in 0..600 {
        w.step();
    }
    let y = min_y(&w, Oil).ok_or("oil vanished")?;
    if y >= 9 {
        return Err(format!("oil did not float up (min y={y})"));
    }
    Ok(())
}

fn wood_chunk_falls_together() -> Result<(), String> {
    let mut w = World::new(12, 12);
    floor(&mut w);
    for y in 1..3 {
        for x in 5..7 {
            w.paint(x, y, Wood);
        }
    }

    for _ in 0..4 {
        w.step();
    }

    if count(&w, Wood) != 4 {
        return Err("wood chunk lost cells while falling".into());
    }
    for y in 5..7 {
        for x in 5..7 {
            if w.get(x, y) != Wood {
                return Err("wood chunk did not fall as a glued 2x2 block".into());
            }
        }
    }
    Ok(())
}

fn structural_chunks_fall_together() -> Result<(), String> {
    for material in [Stone, Glass] {
        let mut w = World::new(12, 12);
        floor(&mut w);
        for y in 1..3 {
            for x in 5..7 {
                w.paint(x, y, material);
            }
        }
        for _ in 0..4 {
            w.step();
        }
        for y in 5..7 {
            for x in 5..7 {
                if w.get(x, y) != material {
                    return Err(format!("{} chunk did not fall together", material.name()));
                }
            }
        }
    }
    Ok(())
}

fn falling_glass_shatters_on_impact() -> Result<(), String> {
    let mut w = World::new(9, 9);
    floor(&mut w);
    w.paint(4, 2, Glass);
    for _ in 0..7 {
        w.step();
    }
    if w.get(4, 7) != BrokenGlass {
        return Err(format!(
            "falling glass did not shatter (got {})",
            w.get(4, 7).name()
        ));
    }
    Ok(())
}

fn wood_chunk_sinks_through_water() -> Result<(), String> {
    let mut w = World::new(12, 14);
    for y in 0..14 {
        w.paint(0, y, Stone);
        w.paint(11, y, Stone);
    }
    floor(&mut w);
    for y in 5..13 {
        for x in 1..11 {
            w.paint(x, y, Water);
        }
    }
    for y in 1..3 {
        for x in 5..7 {
            w.paint(x, y, Wood);
        }
    }

    for _ in 0..10 {
        w.step();
    }

    if count(&w, Wood) != 4 {
        return Err("wood chunk lost cells while sinking".into());
    }
    let y = min_y(&w, Wood).ok_or("wood vanished")?;
    if y < 9 {
        return Err(format!("wood did not sink with weight (min y={y})"));
    }
    Ok(())
}

fn gases_do_not_support_wood() -> Result<(), String> {
    for gas in [Smoke, Steam, Fire] {
        let mut w = World::new(7, 7);
        w.paint(3, 2, Wood);
        w.paint(3, 3, gas);
        w.step();
        if w.get(3, 3) != Wood {
            return Err(format!("{} suspended wood", gas.name()));
        }
    }
    Ok(())
}

fn supported_wood_house_stays_together() -> Result<(), String> {
    let mut w = World::new(14, 12);
    floor(&mut w);
    for y in 7..11 {
        w.paint(3, y, Wood);
        w.paint(9, y, Wood);
    }
    for x in 3..10 {
        w.paint(x, 7, Wood);
    }

    for _ in 0..20 {
        w.step();
    }

    for y in 7..11 {
        if w.get(3, y) != Wood || w.get(9, y) != Wood {
            return Err("supported wood posts collapsed".into());
        }
    }
    for x in 3..10 {
        if w.get(x, 7) != Wood {
            return Err("supported wood beam did not stay glued to posts".into());
        }
    }
    Ok(())
}
fn clear_resets_movement_bookkeeping() -> Result<(), String> {
    let mut w = World::new(7, 7);
    w.paint(3, 1, Wood);
    w.step();

    w.clear();
    w.paint(3, 2, Wood);
    w.step();
    if w.get(3, 3) != Wood {
        return Err("wood was skipped after resetting the world".into());
    }
    Ok(())
}

fn disconnected_wood_section_collapses() -> Result<(), String> {
    let mut w = World::new(14, 14);
    floor(&mut w);
    for x in 4..9 {
        w.paint(x, 4, Wood);
    }
    for y in 5..13 {
        w.paint(6, y, Wood);
    }

    for _ in 0..10 {
        w.step();
    }
    if min_y(&w, Wood) != Some(4) {
        return Err("supported wood section moved before its support was removed".into());
    }

    for y in 5..13 {
        w.paint(6, y, Empty);
    }
    for _ in 0..4 {
        w.step();
    }

    let y = min_y(&w, Wood).ok_or("wood vanished after support removal")?;
    if y <= 4 {
        return Err(format!(
            "disconnected wood section did not collapse (min y={y})"
        ));
    }
    Ok(())
}

fn fire_ignites_wood() -> Result<(), String> {
    let mut w = World::new(16, 12);
    floor(&mut w);
    for y in 5..11 {
        for x in 6..10 {
            w.paint(x, y, Wood);
        }
    }
    w.paint(7, 7, Fire);
    w.set_air_enabled(false);
    let initial = count(&w, Wood);
    for _ in 0..1200 {
        w.step();
    }
    let remaining = count(&w, Wood);
    if remaining >= initial {
        return Err(format!("wood did not burn (remaining={remaining})"));
    }
    Ok(())
}

fn fire_rises_through_smoke_to_ignite_wood() -> Result<(), String> {
    let mut w = World::new(9, 8);
    for y in 1..6 {
        w.paint(3, y, Stone);
        w.paint(5, y, Stone);
    }
    w.paint(4, 5, Stone);
    w.paint(4, 2, Wood);
    w.paint(4, 3, Smoke);
    w.paint(4, 4, Fire);

    for _ in 0..240 {
        w.step();
    }
    if w.get(4, 2) == Wood {
        return Err("smoke blocked fire from igniting wood above it".into());
    }
    Ok(())
}

fn lava_meets_water() -> Result<(), String> {
    let mut w = World::new(12, 12);
    for y in 0..12 {
        w.paint(0, y, Stone);
        w.paint(11, y, Stone);
    }
    floor(&mut w);
    // trap the pair in a small chamber so flow can't separate them first
    for y in 9..11 {
        w.paint(4, y, Stone);
        w.paint(8, y, Stone);
    }
    w.paint(5, 10, Lava);
    w.paint(6, 10, Water);
    for _ in 0..20 {
        w.step();
    }
    let stone = count(&w, Stone) - 2 /*side walls*/ - 1 /*floor row cells in chamber*/;
    if stone < 1 {
        return Err("lava did not solidify into stone next to water".into());
    }
    Ok(())
}

fn acid_dissolves() -> Result<(), String> {
    let mut w = World::new(14, 12);
    floor(&mut w);
    for y in 8..11 {
        for x in 7..10 {
            w.paint(x, y, Wood);
        }
    }
    w.paint(6, 9, Acid);
    let initial = count(&w, Wood);
    for _ in 0..800 {
        w.step();
    }
    let remaining = count(&w, Wood);
    if remaining >= initial {
        return Err(format!(
            "acid did not dissolve wood (remaining={remaining})"
        ));
    }
    Ok(())
}

fn steam_rises() -> Result<(), String> {
    let mut w = World::new(12, 12);
    floor(&mut w);
    w.paint(5, 10, Steam);
    for _ in 0..60 {
        w.step();
    }
    if w.get(5, 10) == Steam {
        return Err("steam did not rise/dissipate".into());
    }
    Ok(())
}

/// Burning wood should smolder into embers and finally leave ash behind, not
/// just vanish into empty space. Ash is sparse (only ~5% of cooled embers leave
/// one), so use a large block and track the peak count over the whole burn.
fn fire_leaves_ash() -> Result<(), String> {
    let mut w = World::new(24, 16);
    floor(&mut w);
    for y in 4..15 {
        for x in 6..18 {
            w.paint(x, y, Wood);
        }
    }
    w.paint(11, 8, Fire);
    w.paint(12, 8, Fire);
    w.set_air_enabled(false);
    let mut peak = 0;
    for _ in 0..4000 {
        w.step();
        peak = peak.max(count(&w, Ash));
    }
    if peak == 0 {
        return Err("burning wood left no ash".into());
    }
    Ok(())
}

/// A real fire breathes dark smoke that rises and dissipates.
fn fire_produces_smoke() -> Result<(), String> {
    let mut w = World::new(16, 16);
    floor(&mut w);
    for y in 7..15 {
        for x in 6..10 {
            w.paint(x, y, Wood);
        }
    }
    w.paint(7, 9, Fire);
    let mut saw_smoke = 0;
    for _ in 0..2000 {
        w.step();
        let c = count(&w, Smoke);
        if c > saw_smoke {
            saw_smoke = c;
        }
    }
    if saw_smoke == 0 {
        return Err("fire produced no smoke".into());
    }
    Ok(())
}

/// Water poured as a tall column should collapse and level out into a flat,
/// shallow layer across the whole basin via hydrostatic lateral velocity.
fn water_levels_out() -> Result<(), String> {
    let mut w = World::new(30, 12);
    for y in 0..12 {
        w.paint(0, y, Stone);
        w.paint(29, y, Stone);
    }
    for x in 0..30 {
        w.paint(x, 11, Stone);
    }
    // a tall column of water hugging the left wall
    for y in 1..11 {
        for x in 1..7 {
            w.paint(x, y, Water);
        }
    }
    for _ in 0..4000 {
        w.step();
    }

    // it must have spread all the way across the basin
    let reached_far = (24..29).any(|x| (1..11).any(|y| w.get(x, y) == Water));
    if !reached_far {
        return Err("water did not spread across the basin".into());
    }

    // and the surface should be flat: every wet column tops out within 2 rows
    let tops: Vec<usize> = (1..29)
        .filter_map(|x| (1..11).filter(|&y| w.get(x, y) == Water).min())
        .collect();
    let (max_top, min_top) = (
        tops.iter().copied().max().unwrap_or(11),
        tops.iter().copied().min().unwrap_or(11),
    );
    if tops.is_empty() {
        return Err("all water vanished".into());
    }
    if max_top - min_top > 2 {
        return Err(format!("surface did not level out ({min_top}..{max_top})"));
    }
    Ok(())
}

/// Drive the real ratatui rendering pipeline (via `TestBackend`) and confirm the
/// grid is drawn with distinct per-material colours. No terminal required.
fn renders_grid_colors() -> Result<(), String> {
    use ratatui::{Terminal, backend::TestBackend};

    // Half-block rendering packs two world rows into each terminal row. An 8-row
    // terminal reserves MAX_STATUS_ROWS for the status bar, leaving 5 grid rows
    // => 10 world rows. Keep a bit of slack below for floor paints.
    let mut w = World::new(16, 10);
    w.paint(1, 2, Sand);
    w.paint(2, 2, Water);
    w.paint(3, 2, Stone);
    w.paint(4, 2, Wood);
    w.paint(5, 2, Oil);
    w.paint(6, 2, Acid);
    w.paint(7, 2, Lava);
    w.paint(8, 2, Steam);
    w.paint(9, 2, Fire);
    w.paint(4, 4, Stone); // floor

    let app = App::default();
    let mut term = Terminal::new(TestBackend::new(16, 8)).map_err(|e| e.to_string())?;
    term.draw(|f| crate::ui::draw(f, &w, &app))
        .map_err(|e| e.to_string())?;
    let buf = term.backend().buffer();

    // A world cell (wx, wy) renders in terminal cell (wx, wy/2). The top half of
    // that cell is the foreground of the ▀ glyph; the bottom half is its bg.
    let wc = |wx: usize, wy: usize| -> (u8, u8, u8) {
        let cx = wx as u16;
        let cy = (wy / 2) as u16;
        let col = if wy.is_multiple_of(2) {
            buf[(cx, cy)].fg
        } else {
            buf[(cx, cy)].bg
        };
        match col {
            ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        }
    };

    // empty cells render as a seamless space: fg == bg
    let empty = wc(0, 0);
    if empty != (8, 10, 16) {
        return Err(format!("empty cell bg {empty:?} != background"));
    }
    let (sr, sg, sb) = wc(1, 2);
    if !(sr > sg && sg > sb && sr >= 200) {
        return Err(format!("sand not yellowish: ({sr},{sg},{sb})"));
    }
    let (wr, wg, wb) = wc(2, 2);
    if !(wb > wr && wb > wg && wb >= 200) {
        return Err(format!("water not bluish: ({wr},{wg},{wb})"));
    }
    let (gr, gg, gb) = wc(3, 2);
    let spread = [gr, gg, gb].iter().max().unwrap() - [gr, gg, gb].iter().min().unwrap();
    if spread > 35 {
        return Err(format!("stone not grey: ({gr},{gg},{gb})"));
    }
    let (fr, _, _) = wc(9, 2);
    if fr < 200 {
        return Err(format!("fire not hot: r={fr}"));
    }
    let (str_, stg, _) = wc(8, 2);
    if str_.abs_diff(stg) > 5 || str_ < 150 {
        return Err(format!("steam not pale grey: ({str_},{stg})"));
    }
    Ok(())
}

/// The material-picker overlay should render its title when open and vanish
/// when closed, and the highlighted row must move with `picker_cursor`.
fn renders_picker() -> Result<(), String> {
    use ratatui::{Terminal, backend::TestBackend};

    let w = World::new(40, 36);
    let mut term = Terminal::new(TestBackend::new(40, 20)).map_err(|e| e.to_string())?;

    let row_text = |buf: &ratatui::buffer::Buffer, y: u16| -> String {
        (0..40)
            .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect::<String>()
    };

    // closed: no "Materials" anywhere on screen (skip reserved status bar rows)
    let app = App::default();
    term.draw(|f| crate::ui::draw(f, &w, &app))
        .map_err(|e| e.to_string())?;
    for y in 0..(20 - crate::ui::MAX_STATUS_ROWS) {
        if row_text(term.backend().buffer(), y).contains("Materials") {
            return Err("picker title shown while closed".into());
        }
    }

    // open: title + the cursor's material name appear
    let app = App {
        picker_open: true,
        // Material::ALL[8] is Fire
        picker_cursor: 8,
        ..App::default()
    };
    term.draw(|f| crate::ui::draw(f, &w, &app))
        .map_err(|e| e.to_string())?;
    let buf = term.backend().buffer();
    let mut found_title = false;
    let mut found_cursor = false;
    for y in 0..20 {
        let r = row_text(buf, y);
        if r.contains("Materials") {
            found_title = true;
        }
        // the highlighted row has a '▶' marker
        if r.contains('▶') {
            found_cursor = true;
        }
    }
    if !found_title {
        return Err("picker title missing when open".into());
    }
    if !found_cursor {
        return Err("picker cursor marker missing".into());
    }
    Ok(())
}

fn brush_options_are_keyboard_accessible() -> Result<(), String> {
    let mut app = App::default();
    let mut world = World::new(12, 12);
    let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));

    app.handle(&key(KeyCode::Char('b')), &mut world);
    if !app.brush_options_open || app.brush_options_cursor != 0 {
        return Err("B did not open brush options on the Shape row".into());
    }
    app.handle(&key(KeyCode::Right), &mut world);
    app.handle(&key(KeyCode::Down), &mut world);
    app.handle(&key(KeyCode::Right), &mut world);
    app.handle(&key(KeyCode::Down), &mut world);
    app.handle(&key(KeyCode::Enter), &mut world);
    if app.brush_shape != BrushShape::Square || app.brush != 3 || !app.brush_erase {
        return Err("brush options did not update shape, radius, and erase mode".into());
    }
    app.handle(&key(KeyCode::Char('b')), &mut world);
    if app.brush_options_open {
        return Err("B did not close brush options".into());
    }
    Ok(())
}

fn ctrl_v_stays_in_paste_mode_on_repeat() -> Result<(), String> {
    let mut app = App {
        clipboard: vec![(Sand, 0, 0, 0)],
        clipboard_size: (1, 1),
        ..App::default()
    };
    let mut world = World::new(4, 4);
    let ctrl_v = Event::Key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
    let ctrl_shift_v = Event::Key(KeyEvent::new(
        KeyCode::Char('V'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));

    app.handle(&ctrl_v, &mut world);
    app.handle(&ctrl_v, &mut world);
    app.handle(&ctrl_shift_v, &mut world);
    if !app.pasting {
        return Err("repeated Ctrl+V canceled paste mode".into());
    }
    Ok(())
}

fn terminal_paste_is_only_text_in_save_input() -> Result<(), String> {
    let mut app = App::default();
    let mut world = World::new(4, 4);

    app.handle(&Event::Paste("sample 2".into()), &mut world);
    if app.scene_menu.open || app.confirm != Confirm::None || app.selected != Sand {
        return Err("terminal paste triggered application shortcuts".into());
    }

    app.scene_menu.open = true;
    app.scene_menu.saving = true;
    app.handle(&Event::Paste("My\nScene/2".into()), &mut world);
    if app.scene_menu.save_name != "MyScene2" {
        return Err("terminal paste was not sanitized into scene-name input".into());
    }
    Ok(())
}

fn clipboard_shortcuts_work_over_overlays() -> Result<(), String> {
    let mut world = World::new(4, 4);
    world.paint(1, 1, Sand);
    let ctrl_c = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

    for mut app in [
        App {
            selection: Some(((1, 1), (1, 1))),
            picker_open: true,
            ..App::default()
        },
        App {
            selection: Some(((1, 1), (1, 1))),
            brush_options_open: true,
            ..App::default()
        },
    ] {
        app.handle(&ctrl_c, &mut world);
        if !app.pasting || app.clipboard.len() != 1 {
            return Err("an overlay intercepted Ctrl+C".into());
        }
    }
    Ok(())
}

fn selection_copy_cut_delete_workflow() -> Result<(), String> {
    let key = |code, modifiers| Event::Key(KeyEvent::new(code, modifiers));
    let click = |column, row| {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };

    let mut world = World::new(10, 10);
    world.paint(1, 1, Sand);
    world.paint(2, 1, Water);
    world.paint(1, 2, Wood);
    world.paint(2, 2, Stone);
    let mut app = App {
        selection: Some(((1, 1), (2, 2))),
        ..App::default()
    };

    app.handle(&key(KeyCode::Char('c'), KeyModifiers::CONTROL), &mut world);
    if !app.pasting
        || app.selection.is_some()
        || app.clipboard_size != (2, 2)
        || app.clipboard.len() != 4
    {
        return Err("Ctrl+C did not dismiss and attach the selected cells to the cursor".into());
    }
    app.handle(&click(4, 2), &mut world);
    app.handle(&click(7, 2), &mut world);
    if world.get(4, 4) != Sand
        || world.get(5, 4) != Water
        || world.get(4, 5) != Wood
        || world.get(5, 5) != Stone
        || world.get(7, 4) != Sand
    {
        return Err("copied selection was not pasted repeatedly at click positions".into());
    }

    app.selection = Some(((1, 1), (2, 2)));
    app.handle(&key(KeyCode::Char('x'), KeyModifiers::CONTROL), &mut world);
    if !app.pasting
        || app.selection.is_some()
        || (1..=2).any(|y| (1..=2).any(|x| world.get(x, y) != Empty))
    {
        return Err("Ctrl+X did not cut the selection and retain it for pasting".into());
    }
    app.handle(&click(1, 1), &mut world);
    if world.get(1, 2) != Sand || world.get(2, 3) != Stone {
        return Err("cut selection could not be pasted".into());
    }

    app.selection = Some(((1, 2), (2, 3)));
    app.handle(&key(KeyCode::Delete, KeyModifiers::NONE), &mut world);
    if app.selection.is_some()
        || app.pasting
        || (2..=3).any(|y| (1..=2).any(|x| world.get(x, y) != Empty))
    {
        return Err("Delete did not clear and dismiss the selection".into());
    }

    app.tool = EditorTool::Select;
    app.selection = Some(((0, 0), (1, 1)));
    app.handle(&key(KeyCode::Char('e'), KeyModifiers::NONE), &mut world);
    app.handle(&key(KeyCode::Down, KeyModifiers::NONE), &mut world);
    app.handle(&key(KeyCode::Enter, KeyModifiers::NONE), &mut world);
    if app.selection.is_some() || app.tool == EditorTool::Select {
        return Err("changing away from Select did not dismiss the selection".into());
    }
    Ok(())
}

fn brush_shapes_erase_and_preview_match() -> Result<(), String> {
    let click = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: 3,
        modifiers: KeyModifiers::NONE,
    });

    let mut circle_world = World::new(12, 12);
    let mut circle = App {
        brush: 1,
        brush_shape: BrushShape::Circle,
        ..App::default()
    };
    circle.handle(&click, &mut circle_world);
    if count(&circle_world, Sand) != 5 {
        return Err("radius-1 circle brush did not paint 5 cells".into());
    }

    let mut square_world = World::new(12, 12);
    for y in 5..=7 {
        for x in 4..=6 {
            square_world.paint(x, y, Stone);
        }
    }
    let mut square = App {
        brush: 1,
        brush_shape: BrushShape::Square,
        brush_erase: true,
        ..App::default()
    };
    square.handle(&click, &mut square_world);
    if count(&square_world, Stone) != 0 {
        return Err("square erase brush did not clear its 3x3 footprint".into());
    }

    let preview = App {
        brush: 1,
        brush_shape: BrushShape::Circle,
        mouse_world: Some((5, 6)),
        ..App::default()
    };
    if !preview.brush_preview_contains(6, 6, 12, 12) || preview.brush_preview_contains(6, 7, 12, 12)
    {
        return Err("circle brush preview does not match the painted footprint".into());
    }
    Ok(())
}

fn quit_and_escape_priority() -> Result<(), String> {
    let mut world = World::new(4, 4);
    let escape = Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let enter = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Esc shows quit confirmation when no overlays are open
    let mut app = App::default();
    if !app.handle(&escape, &mut world) || app.confirm != Confirm::Quit {
        return Err("Esc did not show quit confirmation".into());
    }
    // Enter confirms the quit
    if app.handle(&enter, &mut world) {
        return Err("Enter did not confirm quit".into());
    }

    // Esc closes the material picker without quitting
    let mut app = App {
        picker_open: true,
        ..App::default()
    };
    // First Esc closes picker
    if !app.handle(&escape, &mut world) || app.picker_open {
        return Err("Esc did not close the material picker".into());
    }
    // Second Esc shows quit confirmation
    if !app.handle(&escape, &mut world) || app.confirm != Confirm::Quit {
        return Err("Esc did not show quit confirmation after closing all overlays".into());
    }
    Ok(())
}

fn million_cell_sparse_world_steps() -> Result<(), String> {
    let mut w = World::new(1000, 1000);
    w.paint(500, 10, Sand);
    for _ in 0..80 {
        w.step();
    }
    if count(&w, Sand) != 1 {
        return Err("sand was not conserved in million-cell world".into());
    }
    if min_y(&w, Sand).unwrap_or(0) <= 10 {
        return Err("sand did not move in million-cell world".into());
    }
    Ok(())
}

fn edit_reactivates_settled_cells() -> Result<(), String> {
    let mut w = World::new(8, 8);
    floor(&mut w);
    w.paint(3, 1, Sand);
    for _ in 0..20 {
        w.step();
    }
    if w.get(3, 6) != Sand {
        return Err("sand did not settle above floor".into());
    }

    // This edit touches only the floor row. The active-row scheduler must wake
    // the neighboring row so the settled sand can resume falling.
    w.paint(3, 7, Empty);
    for _ in 0..5 {
        w.step();
    }
    if w.get(3, 7) != Sand {
        return Err("editing below settled sand did not reactivate it".into());
    }
    Ok(())
}

fn saved_scene_round_trips_materials() -> Result<(), String> {
    let mut w = World::new(Material::ALL.len(), 1);
    for (x, &m) in Material::ALL.iter().enumerate() {
        w.paint(x, 0, m);
    }

    let state = crate::scene_manager::SceneState::from_world(&w, "roundtrip".to_string());
    let mut restored = World::new(Material::ALL.len(), 1);
    restored.restore_from(&state);

    for (x, &m) in Material::ALL.iter().enumerate() {
        let got = restored.get(x, 0);
        if got != m {
            return Err(format!(
                "material at x={x} restored as {got:?}, expected {m:?}"
            ));
        }
    }
    Ok(())
}

fn restore_saved_scene_clips_to_current_world() -> Result<(), String> {
    let mut src = World::new(2, 2);
    src.paint(0, 0, Sand);
    src.paint(1, 1, Stone);
    let state = crate::scene_manager::SceneState::from_world(&src, "resize".to_string());

    let mut larger = World::new(4, 4);
    larger.restore_from(&state);
    if larger.get(0, 0) != Sand || larger.get(1, 1) != Stone || larger.get(3, 3) != Empty {
        return Err("larger restore did not preserve/pad expected cells".into());
    }

    let mut smaller = World::new(1, 1);
    smaller.restore_from(&state);
    if smaller.get(0, 0) != Sand {
        return Err("smaller restore did not clip expected top-left cell".into());
    }
    Ok(())
}

fn gunpowder_explosion_damages_its_radius() -> Result<(), String> {
    let mut w = World::new(17, 17);
    let (cx, cy) = (8, 8);
    for y in 3..=13 {
        for x in 3..=13 {
            w.paint(x, y, Stone);
        }
    }
    w.paint(cx, cy, Gunpowder);
    w.paint(cx, cy - 1, Fire);
    w.set_air_enabled(false);
    w.step();

    let inner_x = w.get(cx + 4, cy);
    let inner_y = w.get(cx, cy + 5);
    if inner_x == Stone || inner_y == Stone {
        return Err(format!(
            "blast did not damage material inside its radius (x+4={inner_x:?}, y+5={inner_y:?})"
        ));
    }
    if w.get(cx + 5, cy + 5) != Stone {
        return Err("blast damaged material outside its radius".into());
    }
    Ok(())
}

fn tnt_has_a_large_blast_radius() -> Result<(), String> {
    let mut w = World::new(27, 27);
    let (cx, cy) = (13, 13);
    w.paint(cx + 10, cy, Stone);
    w.paint(cx + 10, cy + 1, Stone);
    w.paint(cx, cy, Tnt);
    w.paint(cx, cy - 1, Fire);
    w.step();

    if w.get(cx + 10, cy) == Stone {
        return Err("TNT did not damage material inside its blast radius".into());
    }
    if w.get(cx + 10, cy + 1) != Stone {
        return Err("TNT damaged material outside its blast radius".into());
    }
    Ok(())
}

fn new_fuels_ignite() -> Result<(), String> {
    for fuel in [Coal, Napalm] {
        let mut w = World::new(5, 5);
        w.paint(2, 3, fuel);
        w.paint(2, 2, Fire);
        for _ in 0..10 {
            w.step();
        }
        if w.get(2, 3) == fuel {
            return Err(format!("{} did not ignite", fuel.name()));
        }
    }
    Ok(())
}

fn fuse_burns_progressively() -> Result<(), String> {
    let mut w = World::new(9, 3);
    // Horizontal fuse line of 5 cells, TNT at the right end, spark at the left.
    for x in 1..=5 {
        w.paint(x, 1, Fuse);
    }
    w.paint(6, 1, Tnt);
    w.paint(0, 1, Fire);

    w.step();

    // After one tick only the spark-adjacent cell is lit; the rest of the fuse
    // is still dormant. The burn front must walk the line over time, not flash
    // the whole component in a single tick.
    if w.get(1, 1) != Fuse || w.life_at(1, 1) == 0 {
        return Err(format!(
            "fuse cell (1, 1) did not light (got {}, life {})",
            w.get(1, 1).name(),
            w.life_at(1, 1)
        ));
    }
    for x in 2..=5 {
        if w.get(x, 1) != Fuse || w.life_at(x, 1) != 0 {
            return Err(format!(
                "fuse cell ({}, 1) lit prematurely (got {}, life {})",
                x,
                w.get(x, 1).name(),
                w.life_at(x, 1)
            ));
        }
    }

    // Let the front travel. The near cell flares to fire while the far end is
    // still dormant fuse, proving the burn is gradual rather than instant.
    for _ in 0..(FUSE_BURN_TICKS + 1) {
        w.step();
    }
    // The near cell has flared (it is now fire, or empty because the flame
    // rose away) while the far end is still dormant fuse — proof the burn is
    // gradual rather than the whole component flashing in one tick.
    if w.get(1, 1) == Fuse {
        return Err(format!(
            "fuse cell (1, 1) did not flare (still {}, life {})",
            w.get(1, 1).name(),
            w.life_at(1, 1)
        ));
    }
    if w.get(5, 1) != Fuse || w.life_at(5, 1) != 0 {
        return Err(format!(
            "far fuse cell (5, 1) burned prematurely (got {}, life {})",
            w.get(5, 1).name(),
            w.life_at(5, 1)
        ));
    }

    // Eventually the whole fuse burns through and the TNT detonates.
    for _ in 0..40 {
        w.step();
    }
    if w.get(6, 1) == Tnt {
        return Err("TNT next to burnt fuse did not detonate".into());
    }

    // Unlit fuse with no heat source stays dormant fuse.
    let mut w2 = World::new(5, 5);
    for x in 1..=4 {
        w2.paint(x, 2, Fuse);
    }
    for _ in 0..10 {
        w2.step();
    }
    for x in 1..=4 {
        if w2.get(x, 2) != Fuse || w2.life_at(x, 2) != 0 {
            return Err(format!(
                "unheated fuse ({}, 2) changed to {} (life {})",
                x,
                w2.get(x, 2).name(),
                w2.life_at(x, 2)
            ));
        }
    }
    Ok(())
}

fn fuse_lit_firework_launches_and_bursts() -> Result<(), String> {
    let mut w = World::new(35, 45);
    // A burning fuse lights the rocket; its seeded timer controls the height.
    w.paint(14, 35, Fire);
    w.paint(15, 35, Fuse);
    w.paint(16, 35, Firework);

    let mut launched = false;
    let mut burst = false;
    for _ in 0..80 {
        w.step();
        launched |= min_y(&w, Firework).is_some_and(|y| y < 35);
        burst |= count(&w, FireworkSpark) > 0;
    }
    if !launched {
        return Err("fuse-lit firework did not launch".into());
    }
    if !burst {
        return Err("launched firework did not produce a spark burst".into());
    }
    Ok(())
}

fn structural_materials_need_sustained_high_heat() -> Result<(), String> {
    for material in [Glass, Stone, Concrete] {
        let (_, delay, product) = material.melt().ok_or("missing melt profile")?;

        // Metal orthogonally cages the sample so it cannot fall, and heat sits only
        // on the diagonals. effective_temp still sees diagonal lava (n8), but
        // react_lava only touches n4 — so Sand/BrokenGlass melt products are not
        // absorbed on the melt tick.
        //   H M H
        //   M X M
        //   H M H
        let build = |heat: Material| {
            let mut w = World::new(3, 3);
            for x in 0..3 {
                for y in 0..3 {
                    let diag = x != 1 && y != 1;
                    let center = x == 1 && y == 1;
                    w.paint(
                        x,
                        y,
                        if center {
                            material
                        } else if diag {
                            heat
                        } else {
                            Metal
                        },
                    );
                }
            }
            w
        };

        let mut ordinary_fire = build(Fire);
        for _ in 0..delay + 10 {
            ordinary_fire.step();
        }
        if ordinary_fire.get(1, 1) != material {
            return Err(format!("{} melted from ordinary fire", material.name()));
        }

        let mut high_heat = build(Lava);
        for _ in 0..delay - 1 {
            high_heat.step();
        }
        if high_heat.get(1, 1) != material {
            return Err(format!("{} melted before its soak delay", material.name()));
        }
        high_heat.step();
        if high_heat.get(1, 1) != product {
            return Err(format!(
                "{} did not melt into {} from sustained lava heat (got {})",
                material.name(),
                product.name(),
                high_heat.get(1, 1).name()
            ));
        }
    }
    Ok(())
}

fn steam_condenses_when_cool() -> Result<(), String> {
    let mut w = World::new(7, 7);
    // Seal a pocket so steam cannot rise away from the cold metal floor.
    for x in 1..6 {
        for y in 1..6 {
            w.paint(x, y, Metal);
        }
    }
    w.paint(3, 3, Steam);
    w.paint(3, 4, Empty);
    w.paint(2, 3, Empty);
    w.paint(4, 3, Empty);
    for _ in 0..200 {
        w.step();
        if count(&w, Water) > 0 {
            return Ok(());
        }
    }
    Err(format!(
        "cool steam did not condense into water (steam={}, empty pocket)",
        count(&w, Steam)
    ))
}

fn heat_conducts_through_metal() -> Result<(), String> {
    let mut w = World::new(7, 3);
    // Lava | Metal | Metal | Wood — heat should soak across the metal bar.
    w.paint(1, 1, Lava);
    w.paint(2, 1, Metal);
    w.paint(3, 1, Metal);
    w.paint(4, 1, Wood);
    for _ in 0..200 {
        w.step();
    }
    // Wood should eventually char once conducted heat + contact path ignites it,
    // or at least the metal far from lava should be warmer than ambient.
    let metal_temp = w.temp_at(3, 1);
    if metal_temp < 100 {
        return Err(format!(
            "heat did not conduct through metal (temp at far metal={metal_temp})"
        ));
    }
    Ok(())
}

fn oil_fire_resists_water() -> Result<(), String> {
    // Ordinary wood fire dies when water touches it.
    let mut wood_fire = World::new(5, 5);
    wood_fire.paint(2, 2, Wood);
    wood_fire.paint(2, 1, Fire);
    wood_fire.paint(1, 1, Water);
    wood_fire.paint(3, 1, Water);
    wood_fire.step();
    if wood_fire.get(2, 1) == Fire {
        return Err("ordinary fire was not quenched by water".into());
    }

    // Greasy fire: water boils off but the flame (or its oil fuel) survives.
    let mut oily = World::new(5, 5);
    oily.paint(2, 2, Oil);
    oily.paint(2, 1, Fire);
    oily.paint(1, 1, Water);
    oily.paint(3, 1, Water);
    oily.step();
    let fire_survived = oily.get(2, 1) == Fire || count(&oily, Fire) > 0;
    let fuel_survived = count(&oily, Oil) > 0;
    if !fire_survived && !fuel_survived {
        return Err("oil fire and fuel were both destroyed by water".into());
    }
    if fire_survived {
        return Ok(());
    }
    // If the flame cell moved, oil must still be present to re-light.
    if !fuel_survived {
        return Err("oil fire was extinguished immediately by water".into());
    }
    Ok(())
}

fn napalm_clings_to_solids() -> Result<(), String> {
    let mut w = World::new(9, 9);
    for x in 0..9 {
        w.paint(x, 8, Stone);
    }
    // A ledge of stone with napalm on top — it should not all drain off quickly.
    for x in 3..6 {
        w.paint(x, 7, Stone);
        w.paint(x, 6, Napalm);
    }
    for _ in 0..40 {
        w.step();
    }
    let still_on_ledge = (3..6).filter(|&x| w.get(x, 6) == Napalm).count();
    if still_on_ledge == 0 {
        return Err("napalm did not cling to the solid ledge".into());
    }
    Ok(())
}

fn salt_dissolves_without_rewriting_water() -> Result<(), String> {
    let mut w = World::new(5, 5);
    w.paint(2, 2, Salt);
    w.paint(2, 3, Water);
    for y in 2..=3 {
        w.paint(1, y, Metal);
        w.paint(3, y, Metal);
    }
    for x in 1..=3 {
        w.paint(x, 4, Metal);
    }
    let water_seed = w.seed_at(2, 3);
    for tick in 0..200 {
        // Keep the chunk active so probabilistic chemistry continues after settling.
        w.paint(0, 0, if tick % 2 == 0 { Metal } else { Empty });
        w.step();
        let salt_remains = (0..w.height).any(|y| (0..w.width).any(|x| w.get(x, y) == Salt));
        if !salt_remains {
            let water_preserved = (0..w.height).any(|y| {
                (0..w.width).any(|x| w.get(x, y) == Water && w.seed_at(x, y) == water_seed)
            });
            if !water_preserved {
                return Err("salt dissolve rewrote or destroyed the water cell".into());
            }
            return Ok(());
        }
    }
    let cells: Vec<_> = (0..w.height)
        .flat_map(|y| (0..w.width).map(move |x| (x, y)))
        .filter(|&(x, y)| w.get(x, y) != Empty)
        .map(|(x, y)| (x, y, w.get(x, y)))
        .collect();
    Err(format!("salt did not dissolve in water: {cells:?}"))
}

fn faucet_emits_consistent_stream() -> Result<(), String> {
    let mut w = World::new(9, 16);
    for x in 0..9 {
        w.paint(x, 15, Stone);
    }
    w.paint(4, 1, Faucet);

    // After a short run the column under the faucet should be a solid stream,
    // not a sparse drip that only appears every few ticks.
    // With persistent velocity, water accelerates and may skip cells, but
    // the stream should still be substantial.
    for _ in 0..30 {
        w.step();
    }
    let column: Vec<Material> = (2..15).map(|y| w.get(4, y)).collect();
    let water_cells = column.iter().filter(|&&m| m == Water).count();
    if water_cells < 7 {
        return Err(format!(
            "faucet stream too sparse under spout (water cells={water_cells}, column={column:?})"
        ));
    }

    // Still producing after the world would otherwise settle.
    let before = count(&w, Water);
    for _ in 0..20 {
        w.step();
    }
    // Either more water accumulated in the basin or the stream is still full.
    let after = count(&w, Water);
    let still_streaming = (2..10).filter(|&y| w.get(4, y) == Water).count() >= 4;
    if after < before && !still_streaming {
        return Err("faucet stopped emitting after the world settled".into());
    }
    Ok(())
}

fn liquid_nitrogen_freezes_and_extinguishes() -> Result<(), String> {
    let mut freezing = World::new(5, 5);
    freezing.paint(2, 3, LiquidNitrogen);
    freezing.paint(2, 2, Water);
    freezing.step();
    if freezing.get(2, 2) != Ice {
        return Err("liquid nitrogen did not freeze water".into());
    }

    let mut quenching = World::new(5, 5);
    quenching.paint(2, 3, LiquidNitrogen);
    quenching.paint(2, 2, Fire);
    quenching.step();
    if quenching.get(2, 3) != Steam || quenching.get(2, 2) == Fire {
        return Err("liquid nitrogen did not extinguish fire".into());
    }
    Ok(())
}

fn c4_blast_respects_structural_materials() -> Result<(), String> {
    let mut w = World::new(31, 31);
    let (cx, cy) = (15, 15);
    // Shelves so unsupported solids don't freefall before the blast resolves.
    // Fragile and resistant targets sit on opposite sides so hard walls do not
    // block LOS to glass/stone under the new inside-out blast model.
    for x in (cx + 6)..=(cx + 10) {
        w.paint(x, cy + 1, Metal);
    }
    for x in (cx - 10)..=(cx - 6) {
        w.paint(x, cy + 1, Metal);
    }
    w.paint(cx + 10, cy, Stone);
    w.paint(cx + 8, cy, Glass);
    w.paint(cx - 7, cy, Metal);
    w.paint(cx - 6, cy, Concrete);
    w.paint(cx, cy, C4);
    w.paint(cx, cy - 1, Fire);
    w.step();

    if w.get(cx + 10, cy) == Stone || w.get(cx + 8, cy) != BrokenGlass {
        return Err("C4 did not damage stone and shatter glass".into());
    }
    if w.get(cx - 7, cy) != Metal || w.get(cx - 6, cy) != Concrete {
        return Err("blast-resistant material was destroyed".into());
    }
    Ok(())
}

fn ice_melt_preserves_cold_water() -> Result<(), String> {
    let mut w = World::new(5, 5);
    // Hot metal conducts into ice without an open flame consuming the cell.
    w.paint(2, 1, Metal);
    w.paint_state(2, 1, (Metal, 0, 0, 400));
    w.paint(2, 2, Ice);
    for _ in 0..80 {
        w.step();
        if w.get(2, 2) == Water {
            let t = w.temp_at(2, 2);
            if t > 15 {
                return Err(format!(
                    "melted water snapped warm (temp={t}, expected near freezing)"
                ));
            }
            return Ok(());
        }
        // Keep the metal hot while heat soaks.
        if w.get(2, 1) == Metal {
            w.paint_state(2, 1, (Metal, 0, w.seed_at(2, 1), 400));
        }
    }
    Err(format!(
        "ice did not melt (cell={}, temp={})",
        w.get(2, 2).name(),
        w.temp_at(2, 2)
    ))
}

fn water_boils_above_100() -> Result<(), String> {
    let mut w = World::new(5, 5);
    // Fixed metal basin so unsupported stone walls cannot collapse under the droplet.
    for x in 1..4 {
        w.paint(x, 3, Metal);
    }
    w.paint(1, 2, Metal);
    w.paint(3, 2, Metal);
    // No fire/lava contact — only stored heat.
    w.paint(2, 2, Water);
    w.paint_state(2, 2, (Water, 0, 0, 140));
    for _ in 0..40 {
        // Re-assert heat each tick so ambient bleed cannot cancel the test.
        if w.get(2, 2) == Water {
            w.paint_state(2, 2, (Water, 0, w.seed_at(2, 2), 140));
        }
        w.step();
        if count(&w, Steam) > 0 || w.get(2, 2) == Steam {
            return Ok(());
        }
    }
    Err(format!(
        "hot water did not boil (cell={}, temp={})",
        w.get(2, 2).name(),
        w.temp_at(2, 2)
    ))
}

fn brief_fire_does_not_vaporize_pool() -> Result<(), String> {
    let mut w = World::new(21, 12);
    for y in 3..=10 {
        w.paint(1, y, Metal);
        w.paint(19, y, Metal);
    }
    for x in 1..=19 {
        w.paint(x, 10, Metal);
    }
    for y in 5..10 {
        for x in 2..19 {
            w.paint(x, y, Water);
        }
    }

    // Briefly replace one surface cell with fire, then let the quenched steam
    // disperse. It may boil nearby water, but must not become a perpetual heat
    // source that propagates through the entire pool.
    w.paint(10, 5, Fire);
    let water_before = count(&w, Water);
    for _ in 0..120 {
        w.step();
        let water = count(&w, Water);
        if water < water_before * 3 / 4 {
            return Err(format!(
                "brief fire vaporized too much of the pool (before={water_before}, remaining={water})"
            ));
        }
    }
    Ok(())
}

fn hot_glass_shatters_in_cold_water() -> Result<(), String> {
    let mut w = World::new(5, 5);
    // Support glass from below; keep water beside it (not under — glass falls into fluids).
    w.paint(2, 3, Metal);
    w.paint(3, 3, Metal);
    w.paint(2, 2, Glass);
    w.paint_state(2, 2, (Glass, 0, 0, 600));
    w.paint(3, 2, Water);
    for _ in 0..10 {
        if w.get(2, 2) == Glass {
            w.paint_state(2, 2, (Glass, 0, 0, 600));
        }
        w.step();
        if w.get(2, 2) == BrokenGlass {
            return Ok(());
        }
    }
    Err(format!(
        "hot glass did not shatter (got {}, temp={})",
        w.get(2, 2).name(),
        w.temp_at(2, 2)
    ))
}

fn sealed_fire_suffocates() -> Result<(), String> {
    // Open air: flame still present after a short run.
    let mut open = World::new(5, 5);
    open.paint(2, 2, Fire);
    for _ in 0..8 {
        open.step();
    }
    if count(&open, Fire) == 0 {
        return Err("open fire died too quickly to compare against sealed fire".into());
    }

    // Fully boxed: every neighbour is solid stone, so the flame has no air.
    let mut sealed = World::new(5, 5);
    for y in 0..5 {
        for x in 0..5 {
            sealed.paint(x, y, Stone);
        }
    }
    sealed.paint(2, 2, Fire);
    for _ in 0..120 {
        sealed.step();
    }
    if count(&sealed, Fire) > 0 {
        return Err("sealed fire did not suffocate".into());
    }
    Ok(())
}

fn blast_moves_sand_outward() -> Result<(), String> {
    let mut w = World::new(21, 11);
    let (cx, cy) = (5, 5);
    // Floor so sand does not just fall away from the measurement.
    for x in 0..21 {
        w.paint(x, 10, Stone);
    }
    // Single sand grain just outside the soft-destruction core, with empty
    // room further out. A pile can pass this check via ordinary avalanche
    // spreading without any blast impulse.
    w.paint(cx + 3, cy, Sand);
    let max_before = (0..w.width)
        .filter(|&x| w.get(x, cy) == Sand)
        .max()
        .unwrap_or(0);
    w.paint(cx, cy, Gunpowder);
    w.paint(cx, cy - 1, Fire);
    // Impulse is applied on the detonation tick; velocity then moves grains.
    for _ in 0..8 {
        w.step();
    }

    let max_after = (0..w.width)
        .filter(|&x| (0..w.height).any(|y| w.get(x, y) == Sand))
        .max()
        .unwrap_or(0);
    if max_after <= max_before {
        return Err(format!(
            "blast did not throw sand outward (max x before={max_before}, after={max_after})"
        ));
    }
    Ok(())
}

fn gunpowder_shatters_nearby_glass() -> Result<(), String> {
    let mut w = World::new(11, 7);
    let (cx, cy) = (3, 3);
    // Support so glass does not freefall before the blast resolves.
    w.paint(cx + 2, cy + 1, Metal);
    w.paint(cx + 2, cy, Glass);
    w.paint(cx, cy, Gunpowder);
    w.paint(cx, cy - 1, Fire);
    w.step();
    if w.get(cx + 2, cy) != BrokenGlass {
        return Err(format!(
            "gunpowder did not shatter nearby glass (got {})",
            w.get(cx + 2, cy).name()
        ));
    }
    Ok(())
}

fn preset_scenes_load_and_run() -> Result<(), String> {
    for scene in Scene::ALL {
        let mut world = World::new(80, 40);
        world.load_scene(scene);
        if count(&world, Empty) == world.width * world.height {
            return Err(format!("{} scene is empty", scene.name()));
        }
        for _ in 0..100 {
            world.step();
        }
    }
    Ok(())
}

fn plant_grows_alongside_water() -> Result<(), String> {
    let mut w = World::new(7, 3);
    // Contain the water beside the plant so they remain adjacent long enough to grow.
    w.paint(2, 1, Plant);
    w.paint_state(3, 1, (Water, 0, 0, 0));
    w.paint(2, 2, Stone);
    w.paint(3, 2, Stone);
    w.paint(4, 1, Stone);
    w.paint(4, 2, Stone);
    let water_count_before = count(&w, Water);
    let plant_count_before = count(&w, Plant);
    for _ in 0..80 {
        w.step();
        if count(&w, Water) != water_count_before {
            return Err("plant growth consumed water".into());
        }
        if count(&w, Plant) > plant_count_before {
            return Ok(());
        }
    }
    Err("plant did not grow into an empty cell next to water".into())
}

fn liquid_nitrogen_thermal_shock() -> Result<(), String> {
    for &(mat, temp_threshold) in &[(Stone, 400), (Concrete, 500), (Metal, 600)] {
        // Thermal-shock scenario: hot material next to LN2.
        let mut shocked = World::new(5, 3);
        shocked.paint(2, 1, mat);
        shocked.paint_state(2, 1, (mat, 0, 0, temp_threshold + 50));
        shocked.paint(2, 2, LiquidNitrogen);
        for _ in 0..80 {
            shocked.step();
            // Keep LN2 fresh so it doesn't all boil away before the shock lands.
            if shocked.get(2, 2) != LiquidNitrogen {
                shocked.paint(2, 2, LiquidNitrogen);
            }
            // Keep the solid hot.
            if shocked.get(2, 1) == mat {
                shocked.paint_state(2, 1, (mat, 0, shocked.seed_at(2, 1), temp_threshold + 50));
            }
            let cell = shocked.get(2, 1);
            if cell == Sand || cell == Empty {
                return Ok(());
            }
        }
    }
    Err("no material experienced thermal shock from liquid nitrogen".into())
}

fn atmos_oxygen_depletion_puts_out_fire() -> Result<(), String> {
    let mut w = World::new(5, 5);
    // Fully sealed box — atmosphere will deplete O₂.
    for y in 0..5 {
        for x in 0..5 {
            w.paint(x, y, Stone);
        }
    }
    // Replace the centre with fire.
    w.paint(2, 2, Fire);
    // Set initial O₂ to a small amount so extinction is quick.
    let fi = w.idx(2, 2);
    w.air_mass_mut()[fi] = AMBIENT_AIR_MASS;
    w.o2_mut()[fi] = 3; // barely any O₂

    for _ in 0..40 {
        w.step();
    }
    if count(&w, Fire) > 0 {
        return Err("fire did not extinguish from O₂ depletion".into());
    }
    // Should have smoke or empty where fire was.
    let cell = w.get(2, 2);
    if cell != Smoke && cell != Empty {
        return Err(format!("fire left unexpected residue: {cell:?}"));
    }
    Ok(())
}

fn atmos_ventilated_fire_burns_longer() -> Result<(), String> {
    // Fire in open air (top edge) should have O₂ replenished via edge venting.
    let mut w = World::new(7, 7);
    w.paint(3, 0, Fire); // right at the open top edge
    for _ in 0..20 {
        w.step();
    }
    let fire_count = count(&w, Fire);
    if fire_count == 0 {
        return Err("ventilated fire died before 20 ticks".into());
    }
    Ok(())
}

fn atmos_disabled_toggle() -> Result<(), String> {
    let mut w = World::new(5, 5);
    // Disable atmosphere.
    assert!(w.atmos_enabled(), "atmos should be enabled by default");
    w.set_air_enabled(false);
    assert!(!w.atmos_enabled(), "atmos should be disabled");
    w.toggle_atmos();
    assert!(w.atmos_enabled(), "atmos should be re-enabled");
    w.toggle_atmos();
    assert!(!w.atmos_enabled(), "atmos should be disabled again");

    // State is preserved while disabled.
    let fi = w.idx(2, 2);
    w.air_mass_mut()[fi] = 123;
    w.o2_mut()[fi] = 45;
    w.set_air_enabled(false);
    for _ in 0..10 {
        w.step();
    }
    assert_eq!(w.air_mass()[fi], 123, "air mass preserved while disabled");
    assert_eq!(w.o2()[fi], 45, "O₂ preserved while disabled");
    Ok(())
}

fn atmos_explosion_adds_heat_and_pressure() -> Result<(), String> {
    let mut w = World::new(11, 11);
    let (cx, cy) = (5, 5);
    w.paint(cx, cy, Tnt);
    w.paint(cx, cy - 1, Fire);

    let fi = w.idx(5, 5);
    let temp_before = w.temp()[fi];
    let mass_before = w.air_mass()[fi];

    w.step(); // explosion happens

    let temp_after = w.temp()[fi];
    let mass_after = w.air_mass()[fi];
    if temp_after <= temp_before {
        return Err(format!(
            "explosion did not add heat (before={temp_before}, after={temp_after})"
        ));
    }
    if mass_after <= mass_before {
        return Err(format!(
            "explosion did not increase air mass (before={mass_before}, after={mass_after})"
        ));
    }
    Ok(())
}

fn atmosphere_round_trip_scene_state() -> Result<(), String> {
    let mut w = World::new(5, 5);
    w.paint(2, 2, Fire);
    let fi = w.idx(2, 2);
    w.air_mass_mut()[fi] = AMBIENT_AIR_MASS;
    w.o2_mut()[fi] = AMBIENT_O2 - 5;
    w.exhaust_mut()[fi] = 10;
    w.fuel_vapor_mut()[fi] = 3;

    let state = crate::scene_manager::SceneState::from_world(&w, "atmos_test".into());
    let mut restored = World::new(5, 5);
    restored.restore_from(&state);

    let ri = restored.idx(2, 2);
    assert_eq!(
        restored.air_mass()[ri],
        AMBIENT_AIR_MASS,
        "air mass round trip"
    );
    assert_eq!(restored.o2()[ri], AMBIENT_O2 - 5, "O₂ round trip");
    assert_eq!(restored.exhaust()[ri], 10, "exhaust round trip");
    assert_eq!(restored.fuel_vapor()[ri], 3, "fuel vapor round trip");
    Ok(())
}

/// The full list of selftest checks: (name, function) pairs.
/// Exposed publicly so `cargo test` can run them without duplication.
pub fn tests() -> &'static [(&'static str, Test)] {
    &[
        ("sand_falls", sand_falls),
        (
            "fast_fall_is_bounded_and_cannot_tunnel",
            fast_fall_is_bounded_and_cannot_tunnel,
        ),
        (
            "water_prefers_route_to_lower_space",
            water_prefers_route_to_lower_space,
        ),
        ("wall_immovable", wall_is_immovable),
        ("water_spreads", water_spreads),
        ("sand_sinks_in_water", sand_sinks_in_water),
        ("oil_floats", oil_floats),
        ("wood_chunk_falls_together", wood_chunk_falls_together),
        (
            "structural_chunks_fall_together",
            structural_chunks_fall_together,
        ),
        (
            "falling_glass_shatters_on_impact",
            falling_glass_shatters_on_impact,
        ),
        (
            "wood_chunk_sinks_through_water",
            wood_chunk_sinks_through_water,
        ),
        ("gases_do_not_support_wood", gases_do_not_support_wood),
        (
            "supported_wood_house_stays_together",
            supported_wood_house_stays_together,
        ),
        (
            "clear_resets_movement_bookkeeping",
            clear_resets_movement_bookkeeping,
        ),
        (
            "disconnected_wood_section_collapses",
            disconnected_wood_section_collapses,
        ),
        ("fire_ignites_wood", fire_ignites_wood),
        (
            "fire_rises_through_smoke_to_ignite_wood",
            fire_rises_through_smoke_to_ignite_wood,
        ),
        ("plant_grows_alongside_water", plant_grows_alongside_water),
        (
            "brush_options_are_keyboard_accessible",
            brush_options_are_keyboard_accessible,
        ),
        (
            "ctrl_v_stays_in_paste_mode_on_repeat",
            ctrl_v_stays_in_paste_mode_on_repeat,
        ),
        (
            "terminal_paste_is_only_text_in_save_input",
            terminal_paste_is_only_text_in_save_input,
        ),
        (
            "clipboard_shortcuts_work_over_overlays",
            clipboard_shortcuts_work_over_overlays,
        ),
        (
            "selection_copy_cut_delete_workflow",
            selection_copy_cut_delete_workflow,
        ),
        (
            "brush_shapes_erase_and_preview_match",
            brush_shapes_erase_and_preview_match,
        ),
        ("quit_and_escape_priority", quit_and_escape_priority),
        ("lava_plus_water_makes_stone", lava_meets_water),
        ("acid_dissolves", acid_dissolves),
        ("steam_rises", steam_rises),
        ("fire_leaves_ash", fire_leaves_ash),
        ("fire_produces_smoke", fire_produces_smoke),
        ("water_levels_out", water_levels_out),
        (
            "gunpowder_explosion_damages_its_radius",
            gunpowder_explosion_damages_its_radius,
        ),
        ("tnt_has_a_large_blast_radius", tnt_has_a_large_blast_radius),
        ("new_fuels_ignite", new_fuels_ignite),
        (
            "fuse_lit_firework_launches_and_bursts",
            fuse_lit_firework_launches_and_bursts,
        ),
        ("fuse_burns_progressively", fuse_burns_progressively),
        (
            "structural_materials_need_sustained_high_heat",
            structural_materials_need_sustained_high_heat,
        ),
        ("steam_condenses_when_cool", steam_condenses_when_cool),
        ("heat_conducts_through_metal", heat_conducts_through_metal),
        ("oil_fire_resists_water", oil_fire_resists_water),
        ("napalm_clings_to_solids", napalm_clings_to_solids),
        (
            "salt_dissolves_without_rewriting_water",
            salt_dissolves_without_rewriting_water,
        ),
        (
            "faucet_emits_consistent_stream",
            faucet_emits_consistent_stream,
        ),
        (
            "liquid_nitrogen_freezes_and_extinguishes",
            liquid_nitrogen_freezes_and_extinguishes,
        ),
        (
            "liquid_nitrogen_thermal_shock",
            liquid_nitrogen_thermal_shock,
        ),
        (
            "c4_blast_respects_structural_materials",
            c4_blast_respects_structural_materials,
        ),
        (
            "million_cell_sparse_world_steps",
            million_cell_sparse_world_steps,
        ),
        (
            "edit_reactivates_settled_cells",
            edit_reactivates_settled_cells,
        ),
        (
            "saved_scene_round_trips_materials",
            saved_scene_round_trips_materials,
        ),
        (
            "restore_saved_scene_clips_to_current_world",
            restore_saved_scene_clips_to_current_world,
        ),
        ("renders_picker", renders_picker),
        ("renders_grid_colors", renders_grid_colors),
        (
            "ice_melt_preserves_cold_water",
            ice_melt_preserves_cold_water,
        ),
        ("water_boils_above_100", water_boils_above_100),
        (
            "brief_fire_does_not_vaporize_pool",
            brief_fire_does_not_vaporize_pool,
        ),
        (
            "hot_glass_shatters_in_cold_water",
            hot_glass_shatters_in_cold_water,
        ),
        ("sealed_fire_suffocates", sealed_fire_suffocates),
        ("blast_moves_sand_outward", blast_moves_sand_outward),
        (
            "gunpowder_shatters_nearby_glass",
            gunpowder_shatters_nearby_glass,
        ),
        ("preset_scenes_load_and_run", preset_scenes_load_and_run),
        (
            "atmos_oxygen_depletion_puts_out_fire",
            atmos_oxygen_depletion_puts_out_fire,
        ),
        (
            "atmos_ventilated_fire_burns_longer",
            atmos_ventilated_fire_burns_longer,
        ),
        ("atmos_disabled_toggle", atmos_disabled_toggle),
        (
            "atmos_explosion_adds_heat_and_pressure",
            atmos_explosion_adds_heat_and_pressure,
        ),
        (
            "atmosphere_round_trip_scene_state",
            atmosphere_round_trip_scene_state,
        ),
    ]
}

pub fn run() -> std::io::Result<()> {
    let tests = tests();
    let mut failed = 0;
    for (name, test) in tests {
        match test() {
            Ok(()) => println!("  PASS  {name}"),
            Err(e) => {
                println!("  FAIL  {name}: {e}");
                failed += 1;
            }
        }
    }

    if failed == 0 {
        println!("\nselftest: all {} checks passed", tests.len());
        Ok(())
    } else {
        println!("\nselftest: {failed}/{} checks FAILED", tests.len());
        std::process::exit(1);
    }
}

#[cfg(test)]
mod test_harness {
    use super::*;

    #[test]
    fn selftest_all() {
        let tests = tests();
        let mut failures: Vec<String> = Vec::new();
        for (name, test) in tests {
            if let Err(e) = test() {
                failures.push(format!("{name}: {e}"));
            }
        }
        if !failures.is_empty() {
            panic!(
                "{} selftest check(s) FAILED:\n{}",
                failures.len(),
                failures.join("\n")
            );
        }
    }
}
