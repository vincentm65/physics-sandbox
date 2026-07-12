use crossterm::event::{Event, KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};

use crate::material::Material;
use crate::scene_manager;
use crate::ui;
use crate::world::{Scene, World};

/// Scene management menu state.
#[derive(Debug, Default)]
pub struct SceneMenu {
    pub open: bool,
    /// Saved scene names (filenames without .json).
    pub scenes: Vec<String>,
    /// Cursor position in the scene list.
    pub cursor: usize,
    /// Saving a new scene — user is typing a name.
    pub saving: bool,
    /// Text being typed for the new scene name.
    pub save_name: String,
}

impl SceneMenu {
    /// Refresh the list of saved scenes from disk.
    pub fn refresh(&mut self) {
        self.scenes = scene_manager::list_scenes().unwrap_or_default();
        self.cursor = 0;
    }

    /// Open the menu and load scene list.
    pub fn open_menu(&mut self) {
        self.open = true;
        self.saving = false;
        self.save_name.clear();
        self.refresh();
    }

    /// Close the menu.
    pub fn close_menu(&mut self) {
        self.open = false;
        self.saving = false;
        self.save_name.clear();
    }

    /// Save the current world as a scene.
    pub fn save_scene(&mut self, world: &World) {
        let name = if self.save_name.is_empty() {
            // Auto-name: "Scene N" where N is next available
            let mut n = 1;
            while self.scenes.iter().any(|s| s == &format!("Scene {n}")) {
                n += 1;
            }
            format!("Scene {n}")
        } else {
            self.save_name.clone()
        };
        let state = scene_manager::SceneState::from_world(world, name);
        if scene_manager::save_scene_state(&state).is_ok() {
            self.refresh();
        }
        self.save_name.clear();
        self.saving = false;
    }

    /// Load a selected scene into the world.
    pub fn load_selected(&self) -> Option<scene_manager::SceneState> {
        self.scenes
            .get(self.cursor)
            .and_then(|name| scene_manager::load_scene_state(name).ok())
    }

    /// Delete the selected scene.
    pub fn delete_selected(&mut self) {
        if let Some(name) = self.scenes.get(self.cursor).cloned() {
            let _ = scene_manager::delete_scene(&name);
            self.refresh();
        }
    }
}

/// Editable interaction state.
#[derive(Debug)]
pub struct App {
    pub selected: Material,
    pub brush: usize,
    pub paused: bool,
    pub quit: bool,
    /// Last painted cell (in world coords) so a fast drag interpolates gaps.
    pub last_mouse: Option<(i32, i32)>,
    /// True between a press and its release — drives drag painting.
    pub drawing: bool,
    /// Material picker overlay open?
    pub picker_open: bool,
    /// Highlighted row in the picker (index into `Material::ALL`).
    pub picker_cursor: usize,
    /// Currently loaded scene.
    pub scene: Scene,
    /// Scene management menu.
    pub scene_menu: SceneMenu,
}

impl Default for App {
    fn default() -> Self {
        Self {
            selected: Material::Sand,
            brush: 2,
            paused: false,
            quit: false,
            last_mouse: None,
            drawing: false,
            picker_open: false,
            picker_cursor: 0,
            scene: Scene::House,
            scene_menu: SceneMenu::default(),
        }
    }
}

impl App {
    /// Apply one input event. Returns `false` to signal the loop should exit.
    pub fn handle(&mut self, ev: &Event, world: &mut World) -> bool {
        match ev {
            Event::Key(k) => self.handle_key(k, world),
            Event::Mouse(me) => self.handle_mouse(me, world),
            _ => true,
        }
    }

    fn handle_key(&mut self, k: &KeyEvent, world: &mut World) -> bool {
        // Esc closes the picker or scene menu without quitting when one is open; Q always quits.
        if self.picker_open && matches!(k.code, KeyCode::Esc) {
            self.picker_open = false;
            return true;
        }
        if self.scene_menu.open && matches!(k.code, KeyCode::Esc) {
            if self.scene_menu.saving {
                self.scene_menu.saving = false;
                self.scene_menu.save_name.clear();
            } else {
                self.scene_menu.close_menu();
            }
            return true;
        }
        if matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
            self.quit = true;
            return false;
        }

        if self.picker_open {
            return self.handle_picker_key(k);
        }

        if self.scene_menu.open {
            return self.handle_scene_menu_key(k, world);
        }

        match k.code {
            KeyCode::Char(' ') | KeyCode::Char('p') => self.paused = !self.paused,
            KeyCode::Char('c') => {
                world.clear();
                self.last_mouse = None;
            }
            KeyCode::Char('n') => {
                self.scene = self.scene.next();
                world.load_scene(self.scene);
                self.last_mouse = None;
            }
            KeyCode::Char('N') => {
                self.scene = self.scene.prev();
                world.load_scene(self.scene);
                self.last_mouse = None;
            }
            // open the scene menu
            KeyCode::Char('s') => {
                if self.scene_menu.open {
                    self.scene_menu.close_menu();
                } else {
                    self.scene_menu.open_menu();
                }
            }
            // open the picker
            KeyCode::Tab | KeyCode::Enter | KeyCode::Char('m') => self.open_picker(),
            // brush size: brackets, +/-, and arrow keys all work
            KeyCode::Char('[') | KeyCode::Char('-') | KeyCode::Left => {
                self.brush = self.brush.saturating_sub(1);
            }
            KeyCode::Char(']') | KeyCode::Char('=') | KeyCode::Right => {
                self.brush = (self.brush + 1).min(MAX_BRUSH);
            }
            KeyCode::Char(c) => {
                if let Some((_, m)) = Material::PALETTE.iter().find(|(k, _)| *k == c) {
                    self.selected = *m;
                }
            }
            _ => {}
        }
        true
    }

    fn handle_picker_key(&mut self, k: &KeyEvent) -> bool {
        let n = Material::ALL.len();
        match k.code {
            KeyCode::Tab | KeyCode::Enter | KeyCode::Char('m') | KeyCode::Char(' ') => {
                self.selected = Material::ALL[self.picker_cursor];
                self.picker_open = false;
            }
            KeyCode::Esc => self.picker_open = false,
            KeyCode::Up | KeyCode::Char('w') | KeyCode::Char('k') => {
                self.picker_cursor = (self.picker_cursor + n - 1) % n;
            }
            KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('j') => {
                self.picker_cursor = (self.picker_cursor + 1) % n;
            }
            KeyCode::Home => self.picker_cursor = 0,
            KeyCode::End => self.picker_cursor = n - 1,
            // number/letter shortcuts select straight from the palette and close
            KeyCode::Char(c) => {
                if let Some((_, m)) = Material::PALETTE.iter().find(|(k, _)| *k == c) {
                    self.selected = *m;
                    self.picker_open = false;
                }
            }
            _ => {}
        }
        true
    }

    fn handle_scene_menu_key(&mut self, k: &KeyEvent, world: &mut World) -> bool {
        if self.scene_menu.saving {
            return self.handle_save_input(k, world);
        }
        let n = self.scene_menu.scenes.len();
        match k.code {
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('w') => {
                if n > 0 {
                    self.scene_menu.cursor = (self.scene_menu.cursor + n - 1) % n;
                }
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('s') => {
                if n > 0 {
                    self.scene_menu.cursor = (self.scene_menu.cursor + 1) % n;
                }
            }
            KeyCode::Char('l') | KeyCode::Enter => {
                if let Some(state) = self.scene_menu.load_selected() {
                    world.restore_from(&state);
                }
            }
            KeyCode::Char('d') => {
                self.scene_menu.delete_selected();
            }
            KeyCode::Char('a') => {
                self.scene_menu.saving = true;
                self.scene_menu.save_name.clear();
            }
            KeyCode::Char('r') => {
                // rename: delete current, enter save mode with same name
                if let Some(name) = self.scene_menu.scenes.get(self.scene_menu.cursor).cloned() {
                    let _ = scene_manager::delete_scene(&name);
                    self.scene_menu.refresh();
                    self.scene_menu.saving = true;
                    self.scene_menu.save_name = name;
                }
            }
            KeyCode::Char('S') => {
                self.scene_menu.save_scene(world);
            }
            _ => {}
        }
        true
    }

    fn handle_save_input(&mut self, k: &KeyEvent, world: &mut World) -> bool {
        match k.code {
            KeyCode::Enter => {
                self.scene_menu.save_scene(world);
            }
            KeyCode::Backspace => {
                self.scene_menu.save_name.pop();
            }
            KeyCode::Char(c) => {
                // Allow alphanumeric, space, underscore, hyphen
                if c.is_alphanumeric() || c == ' ' || c == '_' || c == '-' {
                    self.scene_menu.save_name.push(c);
                }
            }
            _ => {}
        }
        true
    }

    fn open_picker(&mut self) {
        self.picker_open = true;
        if let Some(idx) = Material::ALL.iter().position(|&m| m == self.selected) {
            self.picker_cursor = idx;
        }
    }

    fn handle_mouse(&mut self, me: &MouseEvent, world: &mut World) -> bool {
        match me.kind {
            // scroll wheel resizes the brush (or moves the picker cursor)
            MouseEventKind::ScrollUp => {
                if self.picker_open {
                    let n = Material::ALL.len();
                    self.picker_cursor = (self.picker_cursor + n - 1) % n;
                } else {
                    self.brush = self.brush.saturating_sub(1);
                }
            }
            MouseEventKind::ScrollDown => {
                if self.picker_open {
                    self.picker_cursor = (self.picker_cursor + 1) % Material::ALL.len();
                } else {
                    self.brush = (self.brush + 1).min(MAX_BRUSH);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.picker_open {
                    self.click_picker(me.column, me.row, world);
                } else if self.scene_menu.open {
                    self.click_scene_menu(me.column, me.row, world);
                } else {
                    let (wx, wy) = mouse_to_world(me);
                    paint_brush(world, wx, wy, self.selected, self.brush);
                    self.last_mouse = Some((wx, wy));
                    self.drawing = true;
                }
            }
            // Desktop drag: button-held motion reported as Drag(Left).
            MouseEventKind::Drag(MouseButton::Left) if !self.picker_open => {
                self.paint_drag(me, world);
            }
            // Mobile/Touch: drag motion is often reported as Moved (no button
            // state), so we paint whenever the drawing flag is set.
            MouseEventKind::Moved if !self.picker_open && self.drawing => {
                self.paint_drag(me, world);
            }
            MouseEventKind::Up(_) => {
                self.last_mouse = None;
                self.drawing = false;
            }
            _ => {}
        }
        true
    }

    /// Paint along the drag segment from the last cell to the current one.
    fn paint_drag(&mut self, me: &MouseEvent, world: &mut World) {
        let (wx, wy) = mouse_to_world(me);
        if let Some((px, py)) = self.last_mouse {
            let selected = self.selected;
            let brush = self.brush;
            for_line_points(px, py, wx, wy, |lx, ly| {
                paint_brush(world, lx, ly, selected, brush);
            });
        } else {
            paint_brush(world, wx, wy, self.selected, self.brush);
        }
        self.last_mouse = Some((wx, wy));
    }

    /// Click inside the popup selects that material; click outside closes it.
    fn click_picker(&mut self, col: u16, row: u16, world: &World) {
        let area = ui::picker_rect(world.width as u16, (world.height as u16) / 2 + 1);
        if col >= area.x && col < area.x + area.width && row >= area.y && row < area.y + area.height
        {
            let r = (row - area.y).saturating_sub(1) as usize;
            if r < Material::ALL.len() {
                self.picker_cursor = r;
                self.selected = Material::ALL[r];
                self.picker_open = false;
            }
        } else {
            self.picker_open = false;
        }
    }

    /// Click inside the scene menu popup selects that row; click outside closes it.
    fn click_scene_menu(&mut self, col: u16, row: u16, world: &World) {
        let popup = ui::scene_menu_rect(world.width as u16, (world.height as u16) / 2 + 1);

        if col >= popup.x
            && col < popup.x + popup.width
            && row >= popup.y
            && row < popup.y + popup.height
        {
            // Inside popup — update cursor based on row.
            if row > popup.y && row < popup.y + 9 {
                let r = (row - popup.y - 1) as usize;
                if r < self.scene_menu.scenes.len() {
                    self.scene_menu.cursor = r;
                }
            }
        } else {
            // Outside — close.
            self.scene_menu.close_menu();
        }
    }
}

/// Largest brush radius (cells).
pub const MAX_BRUSH: usize = 8;

/// Half-block rendering maps each terminal row to two world rows.
fn mouse_to_world(me: &MouseEvent) -> (i32, i32) {
    (me.column as i32, (me.row as i32) * 2)
}

/// Paint a filled disk of `radius` centred on `(cx, cy)` (world coords).
fn paint_brush(world: &mut World, cx: i32, cy: i32, m: Material, radius: usize) {
    let r = radius as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                let x = cx + dx;
                let y = cy + dy;
                if x >= 0 && y >= 0 {
                    world.paint(x as usize, y as usize, m);
                }
            }
        }
    }
}

/// Sampled line between two points so a fast drag leaves no gaps.
fn for_line_points(x0: i32, y0: i32, x1: i32, y1: i32, mut f: impl FnMut(i32, i32)) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let steps = dx.unsigned_abs().max(dy.unsigned_abs()).max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        f(
            (x0 as f32 + dx as f32 * t).round() as i32,
            (y0 as f32 + dy as f32 * t).round() as i32,
        );
    }
}
