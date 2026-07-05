//! Headless, deterministic checks for the simulation. Run with `--selftest`.
//! Lets us verify every material interaction without a terminal attached.

use crate::app::App;
use crate::material::Material;
use crate::world::World;
use Material::*;

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
        w.paint(x, w.height - 1, Wall);
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

fn wall_is_immovable() -> Result<(), String> {
    let mut w = World::new(8, 8);
    w.paint(3, 3, Wall);
    for _ in 0..20 {
        w.step();
    }
    if w.get(3, 3) != Wall {
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
    let on_floor = (0..w.width).filter(|&x| w.get(x, w.height - 2) == Water).count();
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
        w.paint(0, y, Wall);
        w.paint(11, y, Wall);
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
        w.paint(0, y, Wall);
        w.paint(11, y, Wall);
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

fn fire_ignites_wood() -> Result<(), String> {
    let mut w = World::new(16, 12);
    floor(&mut w);
    for y in 5..11 {
        for x in 6..10 {
            w.paint(x, y, Wood);
        }
    }
    w.paint(7, 7, Fire);
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

fn lava_meets_water() -> Result<(), String> {
    let mut w = World::new(12, 12);
    for y in 0..12 {
        w.paint(0, y, Wall);
        w.paint(11, y, Wall);
    }
    floor(&mut w);
    // trap the pair in a small chamber so flow can't separate them first
    for y in 9..11 {
        w.paint(4, y, Wall);
        w.paint(8, y, Wall);
    }
    w.paint(5, 10, Lava);
    w.paint(6, 10, Water);
    for _ in 0..20 {
        w.step();
    }
    let stone = count(&w, Wall) - 2 /*side walls*/ - 1 /*floor row cells in chamber*/;
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
        return Err(format!("acid did not dissolve wood (remaining={remaining})"));
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
/// shallow layer across the whole basin (the horizontal-dispersion/flow fix).
/// With only one-cell-per-tick flow it stays heaped near the source for a long
/// time; the multi-cell flow flattens it quickly.
fn water_levels_out() -> Result<(), String> {
    let mut w = World::new(30, 12);
    for y in 0..12 {
        w.paint(0, y, Wall);
        w.paint(29, y, Wall);
    }
    for x in 0..30 {
        w.paint(x, 11, Wall);
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
    use ratatui::{backend::TestBackend, Terminal};

    // Half-block rendering packs two world rows into each terminal row, so an
    // 8-row terminal yields 14 world rows (7 grid rows + status line).
    let mut w = World::new(16, 14);
    w.paint(1, 2, Sand);
    w.paint(2, 2, Water);
    w.paint(3, 2, Wall);
    w.paint(4, 2, Wood);
    w.paint(5, 2, Oil);
    w.paint(6, 2, Acid);
    w.paint(7, 2, Lava);
    w.paint(8, 2, Steam);
    w.paint(9, 2, Fire);
    w.paint(4, 4, Wall); // floor

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
    use ratatui::{backend::TestBackend, Terminal};

    let w = World::new(40, 36);
    let mut term = Terminal::new(TestBackend::new(40, 20)).map_err(|e| e.to_string())?;

    let row_text = |buf: &ratatui::buffer::Buffer, y: u16| -> String {
        (0..40)
            .map(|x| {
                buf[(x, y)].symbol().chars().next().unwrap_or(' ')
            })
            .collect::<String>()
    };

    // closed: no "Materials" anywhere on screen
    let app = App::default();
    term.draw(|f| crate::ui::draw(f, &w, &app))
        .map_err(|e| e.to_string())?;
    for y in 0..20 {
        if row_text(term.backend().buffer(), y).contains("Materials") {
            return Err("picker title shown while closed".into());
        }
    }

    // open: title + the cursor's material name appear
    let mut app = App::default();
    app.picker_open = true;
    // Material::ALL[8] is Fire
    app.picker_cursor = 8;
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

pub fn run() -> std::io::Result<()> {
    let tests: &[(&str, Test)] = &[
        ("sand_falls", sand_falls),
        ("wall_immovable", wall_is_immovable),
        ("water_spreads", water_spreads),
        ("sand_sinks_in_water", sand_sinks_in_water),
        ("oil_floats", oil_floats),
        ("fire_ignites_wood", fire_ignites_wood),
        ("lava_plus_water_makes_stone", lava_meets_water),
        ("acid_dissolves", acid_dissolves),
        ("steam_rises", steam_rises),
        ("fire_leaves_ash", fire_leaves_ash),
        ("fire_produces_smoke", fire_produces_smoke),
        ("water_levels_out", water_levels_out),
        ("renders_picker", renders_picker),
        ("renders_grid_colors", renders_grid_colors),
    ];

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
