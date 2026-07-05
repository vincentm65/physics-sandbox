use std::io::{self, stdout};
use std::time::{Duration, Instant};

use crossterm::{
    cursor,
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

mod app;
mod material;
mod selftest;
mod ui;
mod world;

use app::App;
use world::World;

type Backend = CrosstermBackend<io::Stdout>;

const POLL_WAIT: Duration = Duration::from_millis(15);
const FRAME: Duration = Duration::from_millis(16);

fn main() -> io::Result<()> {
    // `physics-sandbox --selftest` runs headless simulation checks.
    if std::env::args().any(|a| a == "--selftest") {
        return selftest::run();
    }

    let mut app = App::default();
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        event::EnableMouseCapture,
        cursor::Hide
    )?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let size = terminal.size()?;
    let usable = (size.height as usize).saturating_sub(1);
    let mut world = World::new(size.width as usize, usable.saturating_mul(2));

    // Seed a little starting scene so it isn't empty on launch.
    seed_scene(&mut world);

    let result = run(&mut terminal, &mut world, &mut app);

    // Restore the terminal no matter what.
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        event::DisableMouseCapture,
        cursor::Show
    )?;
    terminal.show_cursor()?;
    result
}

fn run(terminal: &mut Terminal<Backend>, world: &mut World, app: &mut App) -> io::Result<()> {
    let mut last = Instant::now();
    let mut step_acc = 0u32;

    loop {
        // Drain any pending input.
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Resize(w, h) => {
                    let usable = (h as usize).saturating_sub(1);
                    world.resize(w as usize, usable.saturating_mul(2));
                }
                ev => {
                    if !app.handle(&ev, world) {
                        return Ok(());
                    }
                }
            }
        }

        let now = Instant::now();
        if now.duration_since(last) >= FRAME {
            let dt = now.duration_since(last);
            last = now;

            if !app.paused {
                // ~30 simulation ticks per second.
                step_acc = step_acc.saturating_add(dt.as_millis() as u32);
                while step_acc >= 33 {
                    world.step();
                    step_acc -= 33;
                }
            }
            terminal.draw(|frame| ui::draw(frame, world, app))?;
        }

        // Block briefly so we don't busy-spin between input bursts.
        if !event::poll(POLL_WAIT)? {
            continue;
        }
        match event::read()? {
            Event::Resize(w, h) => {
                let usable = (h as usize).saturating_sub(1);
                world.resize(w as usize, usable.saturating_mul(2));
            }
            ev => {
                if !app.handle(&ev, world) {
                    return Ok(());
                }
            }
        }
    }
}

fn seed_scene(world: &mut World) {
    let w = world.width;
    let h = world.height;
    if w < 10 || h < 6 {
        return;
    }
    let ground = h - 2;
    // stone floor
    for x in 0..w {
        world.paint(x, ground, material::Material::Wall);
    }
    // a wooden block to burn
    let wx = w / 2;
    for y in (ground - 6)..ground {
        for x in (wx - 2)..(wx + 2) {
            world.paint(x, y, material::Material::Wood);
        }
    }
    // a sand pile on the left
    for y in (ground - 4)..ground {
        for x in 3..9 {
            world.paint(x, y, material::Material::Sand);
        }
    }
}
