use std::io::{self, stdout};
use std::time::{Duration, Instant};

use crossterm::{
    cursor,
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

mod app;
mod blueprint;
mod material;
mod scene_manager;
mod selftest;
mod ui;
mod world;

use app::App;
use world::World;

type Backend = CrosstermBackend<io::Stdout>;

const POLL_WAIT: Duration = Duration::from_millis(15);
const FRAME: Duration = Duration::from_millis(16);

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = stdout();
        let _ = execute!(
            stdout,
            LeaveAlternateScreen,
            event::DisableMouseCapture,
            cursor::Show
        );
    }
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--selftest") {
        return selftest::run();
    }
    if let Some(result) = run_map_command(&args) {
        return result;
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
    let _terminal_guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let size = terminal.size()?;
    let usable = (size.height as usize).saturating_sub(1);
    let mut world = World::new(size.width as usize, usable.saturating_mul(2));
    world.load_scene(app.scene);

    run(&mut terminal, &mut world, &mut app)
}

fn run_map_command(args: &[String]) -> Option<io::Result<()>> {
    let (command, path) = match args {
        [_, command, path] if command == "--map-validate" || command == "--map-build" => {
            (command.as_str(), std::path::Path::new(path))
        }
        _ => return None,
    };

    let result = (|| -> Result<(), String> {
        let blueprint = blueprint::load(path)?;
        let warnings = blueprint.validate()?;
        println!(
            "Valid: {} ({}x{}, {} operations)",
            blueprint.name,
            blueprint.width,
            blueprint.height,
            blueprint.operations.len()
        );
        for warning in warnings {
            println!("Warning: {warning}");
        }
        if command == "--map-build" {
            let state = blueprint.scene_state()?;
            scene_manager::save_scene_state(&state)?;
            println!("Saved scene: {}", state.name);
        }
        Ok(())
    })();

    Some(result.map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error)))
}

fn handle_event(ev: Event, world: &mut World, app: &mut App) -> bool {
    match ev {
        Event::Resize(w, h) => {
            let usable = (h as usize).saturating_sub(1);
            world.resize(w as usize, usable.saturating_mul(2));
            true
        }
        ev => app.handle(&ev, world),
    }
}

fn run(terminal: &mut Terminal<Backend>, world: &mut World, app: &mut App) -> io::Result<()> {
    let mut last = Instant::now();
    let mut step_acc = 0u32;

    loop {
        // Drain any pending input.
        while event::poll(Duration::ZERO)? {
            if !handle_event(event::read()?, world, app) {
                return Ok(());
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
        if !handle_event(event::read()?, world, app) {
            return Ok(());
        }
    }
}
