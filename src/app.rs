use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::material::Material;
use crate::scene_manager;
use crate::ui;
use crate::world::{Scene, World};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EditorTool {
    #[default]
    Brush,
    Line,
    Box,
    FilledBox,
    Circle,
    Select,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MirrorAxis {
    Horizontal,
    Vertical,
}

impl MirrorAxis {
    pub fn name(self) -> &'static str {
        match self {
            Self::Horizontal => "H mirror",
            Self::Vertical => "V mirror",
        }
    }
}

impl EditorTool {
    pub const ALL: [Self; 6] = [
        Self::Brush,
        Self::Line,
        Self::Box,
        Self::FilledBox,
        Self::Circle,
        Self::Select,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Brush => "Brush",
            Self::Line => "Line",
            Self::Box => "Box",
            Self::FilledBox => "Filled box",
            Self::Circle => "Circle",
            Self::Select => "Select",
        }
    }
}

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
    /// Tool picker overlay state.
    pub tool_picker_open: bool,
    pub tool_picker_cursor: usize,
    /// Currently loaded scene.
    pub scene: Scene,
    /// Active drawing tool.
    pub tool: EditorTool,
    /// Drag endpoints for shape placement and preview.
    pub editor_start: Option<(i32, i32)>,
    pub editor_end: Option<(i32, i32)>,
    /// Scene management menu.
    /// Dedicated level-editing mode. Simulation is paused while active.
    pub editor_mode: bool,
    /// 1× shows the whole world; 2× enlarges each cell.
    pub zoom: u8,
    /// View origin used while zoomed in.
    pub camera: (i32, i32),
    /// Screen position and view origin captured for a middle-button pan.
    pub pan_start: Option<((u16, u16), (i32, i32))>,
    /// Optional selection in inclusive world coordinates.
    pub selection: Option<((i32, i32), (i32, i32))>,
    /// Copied cells, including material lifetime and seed.
    pub clipboard: Vec<(Material, u16, u8)>,
    pub clipboard_size: (usize, usize),
    /// Paste copied cells at the next click(s).
    pub pasting: bool,
    /// World cell beneath the mouse, used to preview the clipboard.
    pub paste_cursor: Option<(i32, i32)>,
    /// Mirror edits around the center of the current world.
    pub mirror: Option<MirrorAxis>,
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
            tool_picker_open: false,
            tool_picker_cursor: 0,
            scene: Scene::House,
            tool: EditorTool::default(),
            editor_start: None,
            editor_end: None,
            scene_menu: SceneMenu::default(),
            editor_mode: false,
            zoom: 1,
            camera: (0, 0),
            pan_start: None,
            selection: None,
            clipboard: Vec::new(),
            clipboard_size: (0, 0),
            pasting: false,
            paste_cursor: None,
            mirror: None,
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
        if self.tool_picker_open && matches!(k.code, KeyCode::Esc) {
            self.tool_picker_open = false;
            return true;
        }
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
        if matches!(k.code, KeyCode::Esc) && self.tool != EditorTool::Brush {
            self.tool = EditorTool::Brush;
            self.cancel_shape();
            return true;
        }
        if matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
            self.quit = true;
            return false;
        }

        if self.picker_open {
            return self.handle_picker_key(k);
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('c')) {
            self.copy_selection(world);
            return true;
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('v')) {
            self.pasting = !self.clipboard.is_empty();
            return true;
        }

        if self.tool_picker_open {
            return self.handle_tool_picker_key(k);
        }
        if self.scene_menu.open {
            return self.handle_scene_menu_key(k, world);
        }

        match k.code {
            KeyCode::F(2) => {
                self.editor_mode = !self.editor_mode;
                self.pan_start = None;
                self.paused = self.editor_mode;
                self.pasting = false;
                self.cancel_shape();
            }
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
            KeyCode::Char('r') => {
                world.load_scene(self.scene);
                self.last_mouse = None;
            }
            KeyCode::Char('s') => self.scene_menu.open_menu(),
            KeyCode::Char('e') | KeyCode::Char('E') => self.open_tool_picker(),
            KeyCode::Tab | KeyCode::Enter | KeyCode::Char('m') => self.open_picker(),
            KeyCode::Char('z') if self.editor_mode => self.zoom = 1,
            KeyCode::Char('x') if self.editor_mode => self.zoom = 2,
            KeyCode::Char('i') if self.editor_mode => self.pan(0, -4),
            KeyCode::Char('j') if self.editor_mode => self.pan(-4, 0),
            KeyCode::Char('k') if self.editor_mode => self.pan(0, 4),
            KeyCode::Char('l') if self.editor_mode => self.pan(4, 0),
            KeyCode::Char('h') if self.editor_mode => self.toggle_mirror(MirrorAxis::Horizontal),
            KeyCode::Char('v') if self.editor_mode => self.toggle_mirror(MirrorAxis::Vertical),
            KeyCode::Char('[') | KeyCode::Char('-') | KeyCode::Left => {
                self.brush = self.brush.saturating_sub(1)
            }
            KeyCode::Char(']') | KeyCode::Char('=') | KeyCode::Right => {
                self.brush = (self.brush + 1).min(MAX_BRUSH)
            }
            KeyCode::Char(c) => {
                if let Some((_, m)) = Material::PALETTE.iter().find(|(key, _)| *key == c) {
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

    fn handle_tool_picker_key(&mut self, k: &KeyEvent) -> bool {
        let n = EditorTool::ALL.len();
        match k.code {
            KeyCode::Char('e') | KeyCode::Char('E') | KeyCode::Enter | KeyCode::Char(' ') => {
                self.tool = EditorTool::ALL[self.tool_picker_cursor];
                self.tool_picker_open = false;
                self.cancel_shape();
            }
            KeyCode::Esc => self.tool_picker_open = false,
            KeyCode::Up | KeyCode::Char('w') | KeyCode::Char('k') => {
                self.tool_picker_cursor = (self.tool_picker_cursor + n - 1) % n;
            }
            KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('j') => {
                self.tool_picker_cursor = (self.tool_picker_cursor + 1) % n;
            }
            KeyCode::Home => self.tool_picker_cursor = 0,
            KeyCode::End => self.tool_picker_cursor = n - 1,
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

    fn open_tool_picker(&mut self) {
        self.tool_picker_open = true;
        self.tool_picker_cursor = EditorTool::ALL
            .iter()
            .position(|&tool| tool == self.tool)
            .unwrap_or(0);
        self.cancel_shape();
    }

    fn handle_mouse(&mut self, me: &MouseEvent, world: &mut World) -> bool {
        if self.editor_mode && !self.picker_open && !self.scene_menu.open {
            self.paste_cursor = Some(self.mouse_to_world(me));
        }
        match me.kind {
            // In editor mode, the wheel zooms around the cell beneath the cursor.
            MouseEventKind::ScrollUp => {
                if self.picker_open {
                    let n = Material::ALL.len();
                    self.picker_cursor = (self.picker_cursor + n - 1) % n;
                } else if self.editor_mode {
                    self.set_zoom_at(me, 2);
                } else {
                    self.brush = self.brush.saturating_sub(1);
                }
            }
            MouseEventKind::ScrollDown => {
                if self.picker_open {
                    self.picker_cursor = (self.picker_cursor + 1) % Material::ALL.len();
                } else if self.editor_mode {
                    self.set_zoom_at(me, 1);
                } else {
                    self.brush = (self.brush + 1).min(MAX_BRUSH);
                }
            }
            MouseEventKind::Down(MouseButton::Middle) if self.editor_mode => {
                self.pan_start = Some(((me.column, me.row), self.camera));
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.picker_open {
                    self.click_picker(me.column, me.row, world);
                } else if self.scene_menu.open {
                    self.click_scene_menu(me.column, me.row, world);
                } else {
                    let point = self.mouse_to_world(me);
                    self.drawing = true;
                    self.last_mouse = Some(point);
                    if self.pasting {
                        self.paste_at(world, point);
                    } else if self.tool == EditorTool::Brush {
                        self.paint_brush(world, point.0, point.1);
                    } else {
                        self.editor_start = Some(point);
                        self.editor_end = Some(point);
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if !self.picker_open => {
                self.paint_drag(me, world)
            }
            MouseEventKind::Drag(MouseButton::Middle) if self.editor_mode => self.pan_to(me),
            MouseEventKind::Moved if !self.picker_open && self.drawing => {
                self.paint_drag(me, world)
            }
            MouseEventKind::Up(MouseButton::Middle) => self.pan_start = None,
            MouseEventKind::Up(_) => {
                if self.drawing && !self.pasting && self.tool == EditorTool::Select {
                    self.selection = self.editor_start.zip(self.editor_end);
                } else if self.drawing && !self.pasting && self.tool != EditorTool::Brush {
                    self.paint_shape(world);
                }
                self.last_mouse = None;
                self.editor_start = None;
                self.editor_end = None;
                self.drawing = false;
            }
            _ => {}
        }
        true
    }

    fn paint_drag(&mut self, me: &MouseEvent, world: &mut World) {
        let (wx, wy) = self.mouse_to_world(me);
        if self.pasting {
            return;
        }
        if self.tool != EditorTool::Brush {
            self.editor_end = Some((wx, wy));
        } else if let Some((px, py)) = self.last_mouse {
            for_line_points(px, py, wx, wy, |x, y| self.paint_brush(world, x, y));
        } else {
            self.paint_brush(world, wx, wy);
        }
        self.last_mouse = Some((wx, wy));
    }

    fn cancel_shape(&mut self) {
        self.last_mouse = None;
        self.editor_start = None;
        self.editor_end = None;
        self.drawing = false;
    }
    fn mouse_to_world(&self, me: &MouseEvent) -> (i32, i32) {
        if self.zoom == 2 {
            (
                self.camera.0 + me.column as i32 / 2,
                self.camera.1 + me.row as i32,
            )
        } else {
            (
                self.camera.0 + me.column as i32,
                self.camera.1 + me.row as i32 * 2,
            )
        }
    }

    fn pan(&mut self, dx: i32, dy: i32) {
        self.camera.0 = (self.camera.0 + dx).max(0);
        self.camera.1 = (self.camera.1 + dy).max(0);
    }

    fn set_zoom_at(&mut self, me: &MouseEvent, zoom: u8) {
        let point = self.mouse_to_world(me);
        self.zoom = zoom;
        self.camera = if zoom == 2 {
            (point.0 - me.column as i32 / 2, point.1 - me.row as i32)
        } else {
            (point.0 - me.column as i32, point.1 - me.row as i32 * 2)
        };
        self.camera.0 = self.camera.0.max(0);
        self.camera.1 = self.camera.1.max(0);
    }

    fn pan_to(&mut self, me: &MouseEvent) {
        let Some(((start_x, start_y), origin)) = self.pan_start else {
            return;
        };
        let scale_x = if self.zoom == 2 { 2 } else { 1 };
        let scale_y = if self.zoom == 2 { 1 } else { 2 };
        self.camera.0 = (origin.0 - (me.column as i32 - start_x as i32) / scale_x).max(0);
        self.camera.1 = (origin.1 - (me.row as i32 - start_y as i32) * scale_y).max(0);
    }

    /// Cell that would be replaced at a world position by the active paste.
    pub fn paste_ghost_at(
        &self,
        x: i32,
        y: i32,
        world_width: usize,
        world_height: usize,
    ) -> Option<(Material, u16, u8)> {
        if !self.pasting {
            return None;
        }
        let (anchor_x, anchor_y) = self.paste_cursor?;
        let (width, height) = self.clipboard_size;
        let state_at = |col: i32, row: i32| {
            (col >= 0 && row >= 0 && (col as usize) < width && (row as usize) < height)
                .then(|| {
                    self.clipboard
                        .get(row as usize * width + col as usize)
                        .copied()
                })
                .flatten()
        };
        state_at(x - anchor_x, y - anchor_y).or_else(|| match self.mirror {
            Some(MirrorAxis::Horizontal) => {
                state_at(world_width as i32 - 1 - x - anchor_x, y - anchor_y)
            }
            Some(MirrorAxis::Vertical) => {
                state_at(x - anchor_x, world_height as i32 - 1 - y - anchor_y)
            }
            None => None,
        })
    }

    fn toggle_mirror(&mut self, axis: MirrorAxis) {
        self.mirror = (self.mirror != Some(axis)).then_some(axis);
    }

    fn paint_brush(&self, world: &mut World, cx: i32, cy: i32) {
        paint_brush(world, cx, cy, self.selected, self.brush, self.mirror);
    }

    fn paint_shape(&self, world: &mut World) {
        let (Some(start), Some(end)) = (self.editor_start, self.editor_end) else {
            return;
        };
        let radius = start.0.abs_diff(end.0).max(start.1.abs_diff(end.1)) as i32;
        let (min_x, max_x) = (start.0.min(end.0) - 1, start.0.max(end.0) + 1);
        let (min_y, max_y) = (start.1.min(end.1) - 1, start.1.max(end.1) + 1);
        let (min_x, max_x, min_y, max_y) = if self.tool == EditorTool::Circle {
            (
                start.0 - radius,
                start.0 + radius,
                start.1 - radius,
                start.1 + radius,
            )
        } else {
            (min_x, max_x, min_y, max_y)
        };
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if self.preview_contains(x, y) {
                    paint_cell(world, x, y, self.selected, self.mirror);
                }
            }
        }
    }

    fn copy_selection(&mut self, world: &World) {
        let Some((start, end)) = self.selection else {
            return;
        };
        let min_x = start.0.min(end.0).max(0) as usize;
        let min_y = start.1.min(end.1).max(0) as usize;
        let max_x = start.0.max(end.0).min(world.width.saturating_sub(1) as i32) as usize;
        let max_y = start
            .1
            .max(end.1)
            .min(world.height.saturating_sub(1) as i32) as usize;
        if min_x > max_x || min_y > max_y {
            return;
        }
        self.clipboard.clear();
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                self.clipboard.push(world.cell_state(x, y).unwrap());
            }
        }
        self.clipboard_size = (max_x - min_x + 1, max_y - min_y + 1);
    }

    fn paste_at(&self, world: &mut World, (x, y): (i32, i32)) {
        let (width, height) = self.clipboard_size;
        for row in 0..height {
            for col in 0..width {
                if let Some(&state) = self.clipboard.get(row * width + col) {
                    paint_state(world, x + col as i32, y + row as i32, state, self.mirror);
                }
            }
        }
    }

    pub fn selection_contains(&self, x: i32, y: i32) -> bool {
        let Some((start, end)) = self.selection else {
            return false;
        };
        let (min_x, max_x) = (start.0.min(end.0), start.0.max(end.0));
        let (min_y, max_y) = (start.1.min(end.1), start.1.max(end.1));
        (min_x..=max_x).contains(&x)
            && (min_y..=max_y).contains(&y)
            && (x == min_x || x == max_x || y == min_y || y == max_y)
    }

    pub fn preview_contains(&self, x: i32, y: i32) -> bool {
        let (Some(start), Some(end)) = (self.editor_start, self.editor_end) else {
            return false;
        };
        let min_x = start.0.min(end.0);
        let max_x = start.0.max(end.0);
        let min_y = start.1.min(end.1);
        let max_y = start.1.max(end.1);
        match self.tool {
            EditorTool::Brush => false,
            EditorTool::FilledBox => (min_x..=max_x).contains(&x) && (min_y..=max_y).contains(&y),
            EditorTool::Box => x == min_x || x == max_x || y == min_y || y == max_y,
            EditorTool::Circle => {
                let dx = x - start.0;
                let dy = y - start.1;
                let radius = start.0.abs_diff(end.0).max(start.1.abs_diff(end.1)) as i32;
                dx * dx + dy * dy <= radius * radius
            }
            EditorTool::Line => line_contains(start, end, self.brush + 1, x, y),
            EditorTool::Select => x == min_x || x == max_x || y == min_y || y == max_y,
        }
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

fn paint_brush(
    world: &mut World,
    cx: i32,
    cy: i32,
    m: Material,
    radius: usize,
    mirror: Option<MirrorAxis>,
) {
    let r = radius as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                paint_cell(world, cx + dx, cy + dy, m, mirror);
            }
        }
    }
}

fn paint_cell(world: &mut World, x: i32, y: i32, material: Material, mirror: Option<MirrorAxis>) {
    if x < 0 || y < 0 {
        return;
    }
    world.paint(x as usize, y as usize, material);
    if let Some(axis) = mirror {
        let (mx, my) = match axis {
            MirrorAxis::Horizontal => (world.width as i32 - 1 - x, y),
            MirrorAxis::Vertical => (x, world.height as i32 - 1 - y),
        };
        if mx >= 0 && my >= 0 {
            world.paint(mx as usize, my as usize, material);
        }
    }
}

fn paint_state(
    world: &mut World,
    x: i32,
    y: i32,
    state: (Material, u16, u8),
    mirror: Option<MirrorAxis>,
) {
    if x < 0 || y < 0 {
        return;
    }
    world.paint_state(x as usize, y as usize, state);
    if let Some(axis) = mirror {
        let (mx, my) = match axis {
            MirrorAxis::Horizontal => (world.width as i32 - 1 - x, y),
            MirrorAxis::Vertical => (x, world.height as i32 - 1 - y),
        };
        if mx >= 0 && my >= 0 {
            world.paint_state(mx as usize, my as usize, state);
        }
    }
}

/// Sampled line between two points so a fast drag leaves no gaps.
fn line_contains(start: (i32, i32), end: (i32, i32), thickness: usize, px: i32, py: i32) -> bool {
    let (mut x, mut y) = start;
    let dx = (end.0 - x).abs();
    let sx = if x < end.0 { 1 } else { -1 };
    let dy = -(end.1 - y).abs();
    let sy = if y < end.1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        let half = thickness as i32 / 2;
        if (x - half..x - half + thickness as i32).contains(&px)
            && (y - half..y - half + thickness as i32).contains(&py)
        {
            return true;
        }
        if (x, y) == end {
            return false;
        }
        let e2 = err * 2;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

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
