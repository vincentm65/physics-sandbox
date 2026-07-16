use std::cell::{Ref, RefCell};
use std::collections::HashSet;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::material::Material;
use crate::raster;
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
    Diamond,
    HollowCircle,
    HollowSquare,
    Plus,
    X,
    Select,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BrushShape {
    #[default]
    Circle,
    Square,
    Diamond,
    HollowCircle,
    HollowSquare,
    Plus,
    X,
}

impl BrushShape {
    pub fn name(self) -> &'static str {
        match self {
            Self::Circle => "Circle",
            Self::Square => "Square",
            Self::Diamond => "Diamond",
            Self::HollowCircle => "Hollow Circle",
            Self::HollowSquare => "Hollow Square",
            Self::Plus => "Plus",
            Self::X => "X",
        }
    }

    fn toggle(&mut self) {
        *self = match self {
            Self::Circle => Self::Square,
            Self::Square => Self::Diamond,
            Self::Diamond => Self::HollowCircle,
            Self::HollowCircle => Self::HollowSquare,
            Self::HollowSquare => Self::Plus,
            Self::Plus => Self::X,
            Self::X => Self::Circle,
        };
    }
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
    pub const ALL: [Self; 11] = [
        Self::Brush,
        Self::Line,
        Self::Box,
        Self::FilledBox,
        Self::Circle,
        Self::Diamond,
        Self::HollowCircle,
        Self::HollowSquare,
        Self::Plus,
        Self::X,
        Self::Select,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Brush => "Brush",
            Self::Line => "Line",
            Self::Box => "Box",
            Self::FilledBox => "Filled box",
            Self::Circle => "Circle",
            Self::Diamond => "Diamond",
            Self::HollowCircle => "Hollow Circle",
            Self::HollowSquare => "Hollow Square",
            Self::Plus => "Plus",
            Self::X => "X",
            Self::Select => "Select",
        }
    }
}

/// Scene management menu state.
#[derive(Debug, Default)]
pub struct SceneMenu {
    pub open: bool,
    /// Scene names — built-in scenes are prefixed with `BUILTIN_PREFIX`.
    pub scenes: Vec<String>,
    /// Cursor position in the scene list.
    pub cursor: usize,
    /// Saving a new scene — user is typing a name.
    pub saving: bool,
    /// Text being typed for the new scene name.
    pub save_name: String,
}

/// Prefix that marks built-in scenes in the menu list.
const BUILTIN_PREFIX: &str = "[Built-in] ";

impl SceneMenu {
    /// Refresh the list: built-in scenes first, then user-saved scenes.
    pub fn refresh(&mut self) {
        self.scenes.clear();
        for scene in &Scene::ALL {
            self.scenes
                .push(format!("{BUILTIN_PREFIX}{}", scene.name()));
        }
        if let Ok(mut user) = scene_manager::list_scenes() {
            self.scenes.append(&mut user);
        }
        self.cursor = 0;
    }

    /// Is the scene at the cursor a built-in?
    fn cursor_is_builtin(&self) -> bool {
        self.scenes
            .get(self.cursor)
            .is_some_and(|n| n.starts_with(BUILTIN_PREFIX))
    }

    /// Map a built-in scene name back to its `Scene` variant.
    fn builtin_scene(name: &str) -> Option<Scene> {
        let short = name.strip_prefix(BUILTIN_PREFIX)?;
        Scene::ALL.iter().find(|s| s.name() == short).copied()
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

    /// Save the current world as a scene.  Returns `Ok(())` on success or
    /// a descriptive error string.
    pub fn save_scene(&mut self, world: &World) -> Result<(), String> {
        let name = if self.save_name.is_empty() {
            let mut n = 1;
            while self.scenes.iter().any(|s| s == &format!("Scene {n}")) {
                n += 1;
            }
            format!("Scene {n}")
        } else {
            self.save_name.clone()
        };
        let state = scene_manager::SceneState::from_world(world, name);
        let result = scene_manager::save_scene_state(&state);
        if result.is_ok() {
            self.refresh();
        }
        self.save_name.clear();
        self.saving = false;
        result
    }

    /// Load the selected built-in or user scene.
    pub fn load_selected(&self, world: &mut World) -> Result<(), String> {
        let name = self
            .scenes
            .get(self.cursor)
            .ok_or_else(|| "No scene selected".to_string())?;
        if let Some(scene) = Self::builtin_scene(name) {
            world.load_scene(scene);
            Ok(())
        } else {
            let state = scene_manager::load_scene_state(name)?;
            world.restore_from(&state);
            Ok(())
        }
    }

    /// Delete the selected user scene. Built-in scenes are read-only.
    pub fn delete_selected(&mut self) -> Result<(), String> {
        if self.cursor_is_builtin() {
            return Err("Built-in scenes cannot be deleted".into());
        }
        let name = self
            .scenes
            .get(self.cursor)
            .cloned()
            .ok_or_else(|| "No scene selected".to_string())?;
        let result = scene_manager::delete_scene(&name);
        if result.is_ok() {
            self.refresh();
        }
        result
    }
}

/// Confirmation dialog states.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Confirm {
    #[default]
    None,
    Clear,
    ResetScene,
    SceneDelete,
    SceneOverwrite,
    QuitDirty,
    Quit,
}

impl Confirm {
    pub fn prompt(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Clear => " Clear world?  Enter=Yes  Esc=Cancel ",
            Self::ResetScene => " Reset scene?  Enter=Yes  Esc=Cancel ",
            Self::SceneDelete => " Delete scene?  Enter=Yes  Esc=Cancel ",
            Self::SceneOverwrite => " Overwrite scene?  Enter=Yes  Esc=Cancel ",
            Self::QuitDirty => " Quit with unsaved changes?  Enter=Yes  Esc=Cancel ",
            Self::Quit => " Quit?  Enter=Yes  Esc=Cancel ",
        }
    }
}

/// Maximum number of undo steps kept in memory.
const MAX_UNDO: usize = 32;

type LinePreviewKey = (Option<(i32, i32)>, Option<(i32, i32)>, usize);

#[derive(Debug, Default)]
pub(crate) struct LinePreviewCache {
    key: Option<LinePreviewKey>,
    cells: HashSet<(i32, i32)>,
}

#[derive(Debug)]
struct Run<T> {
    value: T,
    len: u32,
}

#[derive(Debug)]
enum Packed<T> {
    Raw(Vec<T>),
    Runs(Vec<Run<T>>),
}

impl<T: Copy + Eq> Packed<T> {
    fn new(values: &[T]) -> Self {
        let mut runs: Vec<Run<T>> = Vec::new();
        for &value in values {
            if let Some(last) = runs.last_mut()
                && last.value == value
                && last.len < u32::MAX
            {
                last.len += 1;
            } else {
                runs.push(Run { value, len: 1 });
            }
        }
        if runs.len() * std::mem::size_of::<Run<T>>() < std::mem::size_of_val(values) {
            Self::Runs(runs)
        } else {
            Self::Raw(values.to_vec())
        }
    }

    fn expand(&self, len: usize) -> Vec<T> {
        match self {
            Self::Raw(values) => values.clone(),
            Self::Runs(runs) => {
                let mut values = Vec::with_capacity(len);
                for run in runs {
                    values.extend(std::iter::repeat_n(run.value, run.len as usize));
                }
                values
            }
        }
    }
}

/// Memory-efficient internal snapshot used only by undo and redo.
#[derive(Debug)]
pub struct UndoState {
    width: usize,
    height: usize,
    grid: Packed<u8>,
    life: Packed<u16>,
    seed: Packed<u8>,
    vx: Packed<i8>,
    vy: Packed<i8>,
    vy_frac: Packed<i8>,
    y_frac: Packed<i8>,
    vx_frac: Packed<i8>,
    x_frac: Packed<i8>,
    temp: Packed<i16>,
    air_mass: Packed<i16>,
    o2: Packed<i16>,
    exhaust: Packed<i16>,
    fuel_vapor: Packed<i16>,
}

impl UndoState {
    fn from_world(world: &World) -> Self {
        let grid: Vec<u8> = world
            .grid()
            .iter()
            .map(|material| material.to_u8())
            .collect();
        Self {
            width: world.width,
            height: world.height,
            grid: Packed::new(&grid),
            life: Packed::new(world.life()),
            seed: Packed::new(world.seed()),
            vx: Packed::new(world.vx()),
            vy: Packed::new(world.vy()),
            vy_frac: Packed::new(world.vy_frac()),
            y_frac: Packed::new(world.y_frac()),
            vx_frac: Packed::new(world.vx_frac()),
            x_frac: Packed::new(world.x_frac()),
            temp: Packed::new(world.temp()),
            air_mass: Packed::new(world.air_mass()),
            o2: Packed::new(world.o2()),
            exhaust: Packed::new(world.exhaust()),
            fuel_vapor: Packed::new(world.fuel_vapor()),
        }
    }

    fn restore(&self, world: &mut World) {
        let len = self.width * self.height;
        world.restore_from(&scene_manager::SceneState {
            name: String::new(),
            width: self.width,
            height: self.height,
            grid: self.grid.expand(len),
            life: self.life.expand(len),
            seed: self.seed.expand(len),
            vx: self.vx.expand(len),
            vy: self.vy.expand(len),
            vy_frac: self.vy_frac.expand(len),
            y_frac: self.y_frac.expand(len),
            vx_frac: self.vx_frac.expand(len),
            x_frac: self.x_frac.expand(len),
            temp: self.temp.expand(len),
            air_mass: self.air_mass.expand(len),
            o2: self.o2.expand(len),
            exhaust: self.exhaust.expand(len),
            fuel_vapor: self.fuel_vapor.expand(len),
            saved_at: 0,
        });
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AtmosOverlay {
    #[default]
    None,
    Pressure,
    Oxygen,
    Fuel,
    Exhaust,
    Temperature,
}

impl AtmosOverlay {
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Pressure,
            Self::Pressure => Self::Oxygen,
            Self::Oxygen => Self::Fuel,
            Self::Fuel => Self::Exhaust,
            Self::Exhaust => Self::Temperature,
            Self::Temperature => Self::None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "Off",
            Self::Pressure => "Pressure",
            Self::Oxygen => "Oxygen",
            Self::Fuel => "Fuel",
            Self::Exhaust => "Exhaust",
            Self::Temperature => "Temperature",
        }
    }
}

/// Editable interaction state.
#[derive(Debug)]
pub struct App {
    pub selected: Material,
    pub brush: usize,
    pub brush_shape: BrushShape,
    pub brush_erase: bool,
    pub brush_options_open: bool,
    pub brush_options_cursor: usize,
    pub paused: bool,
    pub atmos_overlay: AtmosOverlay,
    pub quit: bool,
    /// Last painted cell (in world coords) so a fast drag interpolates gaps.
    pub last_mouse: Option<(i32, i32)>,
    /// True between a press and its release — drives drag painting.
    pub drawing: bool,
    /// Material picker overlay open?
    pub picker_open: bool,
    /// Highlighted row in the picker (index into `Material::ALL`).
    pub picker_cursor: usize,
    /// Incremental type-ahead filter for the material picker.
    pub picker_query: String,
    /// Tool picker overlay state.
    pub tool_picker_open: bool,
    pub tool_picker_cursor: usize,
    /// Currently loaded built-in scene, used by reset.
    pub scene: Scene,
    /// Name of the currently loaded built-in or user scene.
    pub scene_name: String,
    /// Active drawing tool.
    pub tool: EditorTool,
    /// Drag endpoints for shape placement and preview.
    pub editor_start: Option<(i32, i32)>,
    pub editor_end: Option<(i32, i32)>,
    /// 1× shows the whole world; 2× enlarges each cell.
    pub zoom: u8,
    /// View origin used while zoomed in.
    pub camera: (i32, i32),
    /// Screen position and view origin captured for a middle-button pan.
    pub pan_start: Option<((u16, u16), (i32, i32))>,
    /// Optional selection in inclusive world coordinates.
    pub selection: Option<((i32, i32), (i32, i32))>,
    /// Copied cells, including material lifetime, seed, and temperature.
    pub clipboard: Vec<(Material, u16, u8, i16)>,
    pub clipboard_size: (usize, usize),
    /// Paste copied cells at the next click(s).
    pub pasting: bool,
    /// World cell beneath the mouse, used to preview the clipboard.
    pub paste_cursor: Option<(i32, i32)>,
    /// Cursor world position for display in status bar (editor mode).
    pub mouse_world: Option<(i32, i32)>,
    /// Mirror edits around the center of the current world.
    pub mirror: Option<MirrorAxis>,
    pub scene_menu: SceneMenu,
    /// Whether the world has unsaved changes.
    pub dirty: bool,
    /// Transient status message shown in the UI.
    pub status_msg: Option<String>,
    /// Countdown ticks for status message auto-clear (~60 ticks = 1 s).
    pub status_ticks: u32,
    /// Pending confirmation dialog (None = no confirmation).
    pub confirm: Confirm,
    pub(crate) line_preview_cache: RefCell<LinePreviewCache>,
    /// Compressed undo snapshots.
    pub undo_stack: Vec<UndoState>,
    /// Compressed redo snapshots.
    pub redo_stack: Vec<UndoState>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            selected: Material::Sand,
            brush: 2,
            brush_shape: BrushShape::default(),
            brush_erase: false,
            brush_options_open: false,
            brush_options_cursor: 0,
            paused: false,
            atmos_overlay: AtmosOverlay::None,
            quit: false,
            last_mouse: None,
            drawing: false,
            picker_open: false,
            picker_cursor: 0,
            picker_query: String::new(),
            tool_picker_open: false,
            tool_picker_cursor: 0,
            scene: Scene::House,
            scene_name: Scene::House.name().to_string(),
            tool: EditorTool::default(),
            editor_start: None,
            editor_end: None,
            scene_menu: SceneMenu::default(),
            zoom: 1,
            camera: (0, 0),
            pan_start: None,
            selection: None,
            clipboard: Vec::new(),
            clipboard_size: (0, 0),
            pasting: false,
            paste_cursor: None,
            mouse_world: None,
            mirror: None,
            dirty: false,
            status_msg: None,
            status_ticks: 0,
            confirm: Confirm::None,
            line_preview_cache: RefCell::new(LinePreviewCache::default()),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }
}

impl App {
    /// Apply one input event. Returns `false` to signal the loop should exit.
    pub fn handle(&mut self, ev: &Event, world: &mut World) -> bool {
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.handle_key(k, world),
            Event::Key(_) => true,
            Event::Mouse(me) => self.handle_mouse(me, world),
            Event::Paste(text) => {
                if self.scene_menu.open && self.scene_menu.saving {
                    self.scene_menu.save_name.extend(
                        text.chars()
                            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-')),
                    );
                }
                true
            }
            _ => true,
        }
    }

    /// Age and clear expired status messages.  Call once per frame.
    pub fn tick_status(&mut self) {
        if self.status_ticks > 0 {
            self.status_ticks -= 1;
            if self.status_ticks == 0 {
                self.status_msg = None;
            }
        }
    }

    /// Set a transient status message (auto-clears after ~2 seconds).
    pub fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        let prefix = if is_error { "Error: " } else { "OK: " };
        self.status_msg = Some(format!("{prefix}{}", msg.into()));
        self.status_ticks = 120; // ~2 s at 60 fps
    }

    // ── Snapshot helpers for undo/redo ──────────────────────────────

    /// Capture a compressed snapshot of the current world state.
    fn snapshot(world: &World) -> UndoState {
        UndoState::from_world(world)
    }

    /// Push a snapshot onto the undo stack.
    fn push_undo(&mut self, world: &World) {
        self.undo_stack.push(Self::snapshot(world));
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn handle_key(&mut self, k: &KeyEvent, world: &mut World) -> bool {
        // ── Confirmation-dialog handler ────────────────────────────
        if self.confirm != Confirm::None {
            match k.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let action = self.confirm;
                    self.confirm = Confirm::None;
                    match action {
                        Confirm::Clear => {
                            self.push_undo(world);
                            world.clear();
                            self.last_mouse = None;
                            self.dirty = true;
                            self.set_status("World cleared", false);
                        }
                        Confirm::ResetScene => {
                            self.push_undo(world);
                            world.load_scene(self.scene);
                            self.last_mouse = None;
                            self.dirty = true;
                            self.set_status(format!("Scene {} loaded", self.scene.name()), false);
                        }
                        Confirm::SceneDelete => {
                            let name = self.scene_menu.scenes.get(self.scene_menu.cursor).cloned();
                            match self.scene_menu.delete_selected() {
                                Ok(()) => self.set_status(
                                    format!("Deleted '{}'", name.unwrap_or_default()),
                                    false,
                                ),
                                Err(e) => self.set_status(e, true),
                            }
                        }
                        Confirm::SceneOverwrite => match self.scene_menu.save_scene(world) {
                            Ok(()) => {
                                self.dirty = false;
                                self.set_status("Scene saved", false);
                            }
                            Err(e) => self.set_status(e, true),
                        },
                        Confirm::QuitDirty => {
                            self.quit = true;
                            return false;
                        }
                        Confirm::Quit => {
                            self.quit = true;
                            return false;
                        }
                        Confirm::None => {}
                    }
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.confirm = Confirm::None;
                }
                _ => {}
            }
            return true;
        }

        // ── Ctrl+Z / Ctrl+Y: undo / redo ───────────────────────────
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('z')) {
            if let Some(snap) = self.undo_stack.pop() {
                self.redo_stack.push(Self::snapshot(world));
                snap.restore(world);
                self.dirty = true;
                self.set_status("Undo", false);
            }
            return true;
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('y')) {
            if let Some(snap) = self.redo_stack.pop() {
                self.undo_stack.push(Self::snapshot(world));
                snap.restore(world);
                self.dirty = true;
                self.set_status("Redo", false);
            }
            return true;
        }

        // ---- Esc: close/cancel only, never quit ----
        if matches!(k.code, KeyCode::Esc) {
            if self.brush_options_open {
                self.brush_options_open = false;
                return true;
            }
            if self.tool_picker_open {
                self.tool_picker_open = false;
                return true;
            }
            if self.picker_open {
                self.picker_open = false;
                self.picker_query.clear();
                return true;
            }
            if self.scene_menu.open {
                if self.scene_menu.saving {
                    self.scene_menu.saving = false;
                    self.scene_menu.save_name.clear();
                } else {
                    self.scene_menu.close_menu();
                }
                return true;
            }
            if self.pasting {
                self.pasting = false;
                self.paste_cursor = None;
                self.set_status("Paste cancelled", false);
                return true;
            }
            if self.tool != EditorTool::Brush {
                self.tool = EditorTool::Brush;
                self.selection = None;
                self.cancel_shape();
                return true;
            }
            // Nothing left to close — quit confirmation.
            if self.dirty {
                self.confirm = Confirm::QuitDirty;
            } else {
                self.confirm = Confirm::Quit;
            }
            return true;
        }

        if k.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(k.code, KeyCode::Char('c') | KeyCode::Char('C'))
        {
            if self.copy_selection(world).is_some() {
                self.selection = None;
            }
            return true;
        }
        if k.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(k.code, KeyCode::Char('x') | KeyCode::Char('X'))
        {
            self.cut_selection(world);
            return true;
        }
        if k.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(k.code, KeyCode::Char('v') | KeyCode::Char('V'))
        {
            if self.clipboard.is_empty() {
                self.set_status("Clipboard is empty – copy a selection first (Ctrl+C)", true);
            } else {
                self.selection = None;
                self.pasting = true;
                self.set_status("Click to paste repeatedly  (Esc to cancel)", false);
            }
            return true;
        }

        // Overlay handlers
        if self.brush_options_open {
            return self.handle_brush_options_key(k);
        }
        if self.picker_open {
            return self.handle_picker_key(k);
        }
        if self.tool_picker_open {
            return self.handle_tool_picker_key(k);
        }
        if self.scene_menu.open {
            return self.handle_scene_menu_key(k, world);
        }

        match k.code {
            KeyCode::Delete | KeyCode::Backspace if self.selection.is_some() => {
                self.delete_selection(world);
            }
            KeyCode::Char(' ') | KeyCode::Char('p') => self.paused = !self.paused,
            KeyCode::Char('c') => {
                self.confirm = Confirm::Clear;
            }
            KeyCode::Char('r') => {
                self.confirm = Confirm::ResetScene;
            }
            KeyCode::Char('s') => self.scene_menu.open_menu(),
            KeyCode::Char('b') | KeyCode::Char('B') => {
                self.brush_options_open = true;
                self.brush_options_cursor = 0;
            }
            KeyCode::Char('e') | KeyCode::Char('E') => self.open_tool_picker(),
            KeyCode::Tab | KeyCode::Enter | KeyCode::Char('m') => self.open_picker(),
            KeyCode::Char('a') | KeyCode::Char('A') => {
                world.toggle_atmos();
                self.set_status(
                    format!(
                        "Atmosphere simulation {}",
                        if world.atmos_enabled() { "on" } else { "off" }
                    ),
                    false,
                );
            }
            KeyCode::Char('o') | KeyCode::Char('O') => {
                self.atmos_overlay = self.atmos_overlay.next();
                self.set_status(
                    format!("Atmosphere overlay: {}", self.atmos_overlay.name()),
                    false,
                );
            }
            KeyCode::Char('z') => self.zoom = 1,
            KeyCode::Char('x') => self.zoom = 2,
            KeyCode::Char('i') => self.pan(0, -4, world),
            KeyCode::Char('j') => self.pan(-4, 0, world),
            KeyCode::Char('k') => self.pan(0, 4, world),
            KeyCode::Char('l') => self.pan(4, 0, world),
            KeyCode::Char('h') => self.toggle_mirror(MirrorAxis::Horizontal),
            KeyCode::Char('v') => self.toggle_mirror(MirrorAxis::Vertical),
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
    fn handle_brush_options_key(&mut self, k: &KeyEvent) -> bool {
        match k.code {
            KeyCode::Char('b') | KeyCode::Char('B') | KeyCode::Esc => {
                self.brush_options_open = false;
            }
            KeyCode::Up | KeyCode::Char('w') | KeyCode::Char('k') => {
                self.brush_options_cursor = (self.brush_options_cursor + 2) % 3;
            }
            KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('j') => {
                self.brush_options_cursor = (self.brush_options_cursor + 1) % 3;
            }
            KeyCode::Left | KeyCode::Char('[') | KeyCode::Char('-') => {
                self.change_brush_option(false);
            }
            KeyCode::Right | KeyCode::Char(']') | KeyCode::Char('=') => {
                self.change_brush_option(true);
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_brush_option(),
            KeyCode::Home => self.brush_options_cursor = 0,
            KeyCode::End => self.brush_options_cursor = 2,
            _ => {}
        }
        true
    }

    fn change_brush_option(&mut self, increase: bool) {
        match self.brush_options_cursor {
            0 => self.brush_shape.toggle(),
            1 if increase => self.brush = (self.brush + 1).min(MAX_BRUSH),
            1 => self.brush = self.brush.saturating_sub(1),
            2 => self.brush_erase = !self.brush_erase,
            _ => {}
        }
    }

    fn toggle_brush_option(&mut self) {
        match self.brush_options_cursor {
            0 => self.brush_shape.toggle(),
            1 => self.brush = (self.brush + 1) % (MAX_BRUSH + 1),
            2 => self.brush_erase = !self.brush_erase,
            _ => {}
        }
    }

    fn handle_picker_key(&mut self, k: &KeyEvent) -> bool {
        let n = Material::ALL.len();
        match k.code {
            KeyCode::Tab | KeyCode::Enter | KeyCode::Char(' ') => {
                self.selected = Material::ALL[self.picker_cursor];
                self.picker_open = false;
                self.picker_query.clear();
            }
            KeyCode::Esc => {
                self.picker_open = false;
                self.picker_query.clear();
            }
            KeyCode::Up => {
                self.picker_cursor = (self.picker_cursor + n - 1) % n;
                self.picker_query.clear();
            }
            KeyCode::Down => {
                self.picker_cursor = (self.picker_cursor + 1) % n;
                self.picker_query.clear();
            }
            KeyCode::Home => {
                self.picker_cursor = 0;
                self.picker_query.clear();
            }
            KeyCode::End => {
                self.picker_cursor = n - 1;
                self.picker_query.clear();
            }
            KeyCode::Backspace => {
                self.picker_query.pop();
                if !self.picker_query.is_empty() {
                    self.apply_picker_query();
                }
            }
            // Alphanumerics (and spaces) accumulate as a type-ahead query and
            // jump the cursor to the first matching material name. Digits are
            // part of the query so names like "C4"/"TNT" work; palette digit
            // shortcuts still work outside the picker.
            KeyCode::Char(c) if c.is_ascii_alphanumeric() || c == ' ' => {
                self.picker_query.push(c.to_ascii_lowercase());
                self.apply_picker_query();
            }
            _ => {}
        }
        true
    }

    /// Move `picker_cursor` to the first material whose name starts with the
    /// current type-ahead query (case-insensitive; non-alphanumerics ignored).
    fn apply_picker_query(&mut self) {
        let query: String = self
            .picker_query
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if query.is_empty() {
            return;
        }
        if let Some(idx) = Material::ALL.iter().position(|m| {
            let name: String = m
                .name()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_lowercase())
                .collect();
            name.starts_with(&query)
        }) {
            self.picker_cursor = idx;
        }
    }

    fn handle_tool_picker_key(&mut self, k: &KeyEvent) -> bool {
        let n = EditorTool::ALL.len();
        match k.code {
            KeyCode::Char('e') | KeyCode::Char('E') | KeyCode::Enter | KeyCode::Char(' ') => {
                self.tool = EditorTool::ALL[self.tool_picker_cursor];
                if self.tool != EditorTool::Select {
                    self.selection = None;
                }
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
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('w') if n > 0 => {
                self.scene_menu.cursor = (self.scene_menu.cursor + n - 1) % n;
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('s') if n > 0 => {
                self.scene_menu.cursor = (self.scene_menu.cursor + 1) % n;
            }
            KeyCode::Char('l') | KeyCode::Enter => {
                let selected = self.scene_menu.scenes.get(self.scene_menu.cursor).cloned();
                self.push_undo(world);
                match self.scene_menu.load_selected(world) {
                    Ok(()) => {
                        if let Some(name) = selected {
                            if let Some(scene) = SceneMenu::builtin_scene(&name) {
                                self.scene = scene;
                            }
                            self.scene_name = name
                                .strip_prefix(BUILTIN_PREFIX)
                                .unwrap_or(&name)
                                .to_string();
                        }
                        self.dirty = false;
                        self.set_status(format!("Scene {} loaded", self.scene_name), false);
                        self.scene_menu.close_menu();
                    }
                    Err(e) => {
                        self.undo_stack.pop();
                        self.set_status(e, true);
                    }
                }
            }
            KeyCode::Char('d') => {
                if !self.scene_menu.scenes.is_empty() {
                    self.confirm = Confirm::SceneDelete;
                } else {
                    self.set_status("No scene selected", true);
                }
            }
            KeyCode::Char('a') => {
                self.scene_menu.saving = true;
                self.scene_menu.save_name.clear();
            }
            KeyCode::Char('r') => {
                // Start from the selected name without deleting the original.
                if let Some(name) = self.scene_menu.scenes.get(self.scene_menu.cursor).cloned() {
                    self.scene_menu.saving = true;
                    self.scene_menu.save_name = name;
                }
            }
            KeyCode::Char('S') => {
                if self.scene_menu.cursor_is_builtin() {
                    self.set_status("Built-in scenes cannot be overwritten", true);
                } else if let Some(name) =
                    self.scene_menu.scenes.get(self.scene_menu.cursor).cloned()
                {
                    self.scene_menu.save_name = name;
                    self.confirm = Confirm::SceneOverwrite;
                } else {
                    self.set_status("No scene selected", true);
                }
            }
            _ => {}
        }
        true
    }

    fn handle_save_input(&mut self, k: &KeyEvent, world: &mut World) -> bool {
        match k.code {
            KeyCode::Enter => {
                let name = if self.scene_menu.save_name.is_empty() {
                    let mut n = 1;
                    while self
                        .scene_menu
                        .scenes
                        .iter()
                        .any(|s| s == &format!("Scene {n}"))
                    {
                        n += 1;
                    }
                    format!("Scene {n}")
                } else {
                    self.scene_menu.save_name.clone()
                };
                // Check if overwriting existing scene
                if self.scene_menu.scenes.contains(&name) {
                    self.confirm = Confirm::SceneOverwrite;
                    return true;
                }
                match self.scene_menu.save_scene(world) {
                    Ok(()) => {
                        self.dirty = false;
                        self.set_status("Scene saved", false);
                    }
                    Err(e) => self.set_status(e, true),
                }
            }
            KeyCode::Backspace => {
                self.scene_menu.save_name.pop();
            }
            KeyCode::Char(c)
                // Allow alphanumeric, space, underscore, hyphen
                if (c.is_alphanumeric() || c == ' ' || c == '_' || c == '-') => {
                    self.scene_menu.save_name.push(c);
                }
            _ => {}
        }
        true
    }

    fn open_picker(&mut self) {
        self.picker_open = true;
        self.picker_query.clear();
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
        if self.confirm != Confirm::None || self.brush_options_open {
            return true;
        }
        if !self.picker_open && !self.scene_menu.open && !self.tool_picker_open {
            self.mouse_world = Some(self.mouse_to_world(me));
            self.paste_cursor = self.mouse_world;
        }
        match me.kind {
            // Scroll wheel zooms around the cell beneath the cursor.
            // Ctrl+scroll adjusts brush size instead.
            MouseEventKind::ScrollUp => {
                if self.picker_open {
                    let n = Material::ALL.len();
                    self.picker_cursor = (self.picker_cursor + n - 1) % n;
                } else if self.tool_picker_open {
                    let n = EditorTool::ALL.len();
                    self.tool_picker_cursor = (self.tool_picker_cursor + n - 1) % n;
                } else if self.scene_menu.open {
                    let n = self.scene_menu.scenes.len();
                    if n > 0 {
                        self.scene_menu.cursor = (self.scene_menu.cursor + n - 1) % n;
                    }
                } else if me.modifiers.contains(KeyModifiers::CONTROL) {
                    self.brush = (self.brush + 1).min(MAX_BRUSH);
                } else {
                    self.set_zoom_at(me, 2, world);
                }
            }
            MouseEventKind::ScrollDown => {
                if self.picker_open {
                    self.picker_cursor = (self.picker_cursor + 1) % Material::ALL.len();
                } else if self.tool_picker_open {
                    let n = EditorTool::ALL.len();
                    self.tool_picker_cursor = (self.tool_picker_cursor + 1) % n;
                } else if self.scene_menu.open {
                    let n = self.scene_menu.scenes.len();
                    if n > 0 {
                        self.scene_menu.cursor = (self.scene_menu.cursor + 1) % n;
                    }
                } else if me.modifiers.contains(KeyModifiers::CONTROL) {
                    self.brush = self.brush.saturating_sub(1);
                } else {
                    self.set_zoom_at(me, 1, world);
                }
            }
            MouseEventKind::Down(MouseButton::Middle)
                if !self.picker_open && !self.scene_menu.open && !self.tool_picker_open =>
            {
                self.pan_start = Some(((me.column, me.row), self.camera));
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.picker_open {
                    self.click_picker(me.column, me.row, world);
                } else if self.scene_menu.open {
                    self.click_scene_menu(me.column, me.row, world);
                } else if self.tool_picker_open {
                    // Tool selection is keyboard-only; do not paint through the modal.
                } else {
                    let point = self.mouse_to_world(me);
                    self.drawing = true;
                    self.last_mouse = Some(point);
                    if self.pasting {
                        self.push_undo(world);
                        self.paste_at(world, point);
                        self.dirty = true;
                    } else if self.tool == EditorTool::Brush {
                        self.push_undo(world);
                        self.paint_brush(world, point.0, point.1);
                        self.dirty = true;
                    } else {
                        if self.tool == EditorTool::Select {
                            self.selection = None;
                        }
                        self.editor_start = Some(point);
                        self.editor_end = Some(point);
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left)
                if !self.picker_open && !self.scene_menu.open && !self.tool_picker_open =>
            {
                self.paint_drag(me, world)
            }
            MouseEventKind::Drag(MouseButton::Middle)
                if !self.picker_open && !self.scene_menu.open && !self.tool_picker_open =>
            {
                self.pan_to(me, world)
            }
            MouseEventKind::Moved
                if !self.picker_open
                    && !self.scene_menu.open
                    && !self.tool_picker_open
                    && self.drawing =>
            {
                self.paint_drag(me, world)
            }
            MouseEventKind::Up(MouseButton::Middle) => self.pan_start = None,
            MouseEventKind::Up(_) => {
                if self.drawing && !self.pasting && self.tool == EditorTool::Select {
                    self.selection = self.editor_start.zip(self.editor_end);
                } else if self.drawing && !self.pasting && self.tool != EditorTool::Brush {
                    self.push_undo(world);
                    self.paint_shape(world);
                    self.dirty = true;
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
            self.dirty = true;
        } else {
            self.push_undo(world);
            self.paint_brush(world, wx, wy);
            self.dirty = true;
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

    fn pan(&mut self, dx: i32, dy: i32, world: &World) {
        self.camera.0 = (self.camera.0 + dx).max(0);
        self.camera.1 = (self.camera.1 + dy).max(0);
        self.clamp_camera(world);
    }

    fn clamp_camera(&mut self, world: &World) {
        self.camera.0 = self
            .camera
            .0
            .min((world.width as i32).saturating_sub(1).max(0));
        self.camera.1 = self
            .camera
            .1
            .min((world.height as i32).saturating_sub(1).max(0));
    }

    fn set_zoom_at(&mut self, me: &MouseEvent, zoom: u8, world: &World) {
        let point = self.mouse_to_world(me);
        self.zoom = zoom;
        self.camera = if zoom == 2 {
            (point.0 - me.column as i32 / 2, point.1 - me.row as i32)
        } else {
            (point.0 - me.column as i32, point.1 - me.row as i32 * 2)
        };
        self.camera.0 = self.camera.0.max(0);
        self.camera.1 = self.camera.1.max(0);
        self.clamp_camera(world);
    }

    fn pan_to(&mut self, me: &MouseEvent, world: &World) {
        let Some(((start_x, start_y), origin)) = self.pan_start else {
            return;
        };
        let scale_x = if self.zoom == 2 { 2 } else { 1 };
        let scale_y = if self.zoom == 2 { 1 } else { 2 };
        self.camera.0 = (origin.0 - (me.column as i32 - start_x as i32) / scale_x).max(0);
        self.camera.1 = (origin.1 - (me.row as i32 - start_y as i32) * scale_y).max(0);
        self.clamp_camera(world);
    }

    /// Cell that would be replaced at a world position by the active paste.
    pub fn paste_ghost_at(
        &self,
        x: i32,
        y: i32,
        world_width: usize,
        world_height: usize,
    ) -> Option<(Material, u16, u8, i16)> {
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
        let material = if self.brush_erase {
            Material::Empty
        } else {
            self.selected
        };
        paint_brush(
            world,
            cx,
            cy,
            material,
            self.brush,
            self.brush_shape,
            self.mirror,
        );
    }

    fn paint_shape(&self, world: &mut World) {
        let (Some(start), Some(end)) = (self.editor_start, self.editor_end) else {
            return;
        };
        let radius = start.0.abs_diff(end.0).max(start.1.abs_diff(end.1)) as i32;
        let (min_x, max_x) = (start.0.min(end.0) - 1, start.0.max(end.0) + 1);
        let (min_y, max_y) = (start.1.min(end.1) - 1, start.1.max(end.1) + 1);
        let (min_x, max_x, min_y, max_y) = if matches!(
            self.tool,
            EditorTool::Circle
                | EditorTool::Diamond
                | EditorTool::HollowCircle
                | EditorTool::HollowSquare
                | EditorTool::Plus
                | EditorTool::X
        ) {
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

    fn selection_bounds(&self, world: &World) -> Option<(usize, usize, usize, usize)> {
        let (start, end) = self.selection?;
        if world.width == 0 || world.height == 0 {
            return None;
        }
        let min_x = start.0.min(end.0).max(0);
        let min_y = start.1.min(end.1).max(0);
        let max_x = start.0.max(end.0).min(world.width.saturating_sub(1) as i32);
        let max_y = start
            .1
            .max(end.1)
            .min(world.height.saturating_sub(1) as i32);
        (min_x <= max_x && min_y <= max_y).then_some((
            min_x as usize,
            min_y as usize,
            max_x as usize,
            max_y as usize,
        ))
    }

    fn copy_selection(&mut self, world: &World) -> Option<usize> {
        let Some((min_x, min_y, max_x, max_y)) = self.selection_bounds(world) else {
            let message = if self.selection.is_some() {
                "Selection is outside world bounds"
            } else {
                "Nothing selected – use Select tool first"
            };
            self.set_status(message, true);
            return None;
        };
        self.clipboard.clear();
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if let Some(state) = world.cell_state(x, y) {
                    self.clipboard.push(state);
                }
            }
        }
        self.clipboard_size = (max_x - min_x + 1, max_y - min_y + 1);
        let count = self.clipboard.len();
        self.pasting = true;
        self.paste_cursor = self.mouse_world;
        self.set_status(
            format!("Copied {count} cells – click to paste repeatedly  (Esc to cancel)"),
            false,
        );
        Some(count)
    }

    fn clear_selection_cells(&self, world: &mut World) -> Option<usize> {
        let (min_x, min_y, max_x, max_y) = self.selection_bounds(world)?;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                world.paint(x, y, Material::Empty);
            }
        }
        Some((max_x - min_x + 1) * (max_y - min_y + 1))
    }

    fn delete_selection(&mut self, world: &mut World) {
        if self.selection_bounds(world).is_none() {
            self.set_status("Nothing selected – use Select tool first", true);
            return;
        }
        self.push_undo(world);
        let count = self.clear_selection_cells(world).unwrap_or(0);
        self.selection = None;
        self.pasting = false;
        self.paste_cursor = None;
        self.dirty = true;
        self.set_status(format!("Deleted {count} cells"), false);
    }

    fn cut_selection(&mut self, world: &mut World) {
        let Some(count) = self.copy_selection(world) else {
            return;
        };
        self.push_undo(world);
        self.clear_selection_cells(world);
        self.selection = None;
        self.dirty = true;
        self.set_status(
            format!("Cut {count} cells – click to paste repeatedly  (Esc to cancel)"),
            false,
        );
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

    pub fn brush_preview_contains(
        &self,
        x: i32,
        y: i32,
        world_width: usize,
        world_height: usize,
    ) -> bool {
        if self.tool != EditorTool::Brush
            || self.pasting
            || self.brush_options_open
            || self.picker_open
            || self.tool_picker_open
            || self.scene_menu.open
            || self.confirm != Confirm::None
        {
            return false;
        }
        let Some((cx, cy)) = self.mouse_world else {
            return false;
        };
        let radius = self.brush as i32;
        let contains = |center_x: i32, center_y: i32| {
            brush_offset_contains(self.brush_shape, radius, x - center_x, y - center_y)
        };
        contains(cx, cy)
            || match self.mirror {
                Some(MirrorAxis::Horizontal) => contains(world_width as i32 - 1 - cx, cy),
                Some(MirrorAxis::Vertical) => contains(cx, world_height as i32 - 1 - cy),
                None => false,
            }
    }

    /// Preview highlight colour for the active brush or shape.
    pub fn preview_color(&self, tick: u64) -> ratatui::style::Color {
        if self.brush_erase && self.tool == EditorTool::Brush {
            ratatui::style::Color::Rgb(255, 120, 100)
        } else {
            self.selected.color(0, 128, tick)
        }
    }

    pub(crate) fn line_preview_cells(&self) -> Option<Ref<'_, HashSet<(i32, i32)>>> {
        if self.tool != EditorTool::Line {
            return None;
        }
        let key = (self.editor_start, self.editor_end, self.brush);
        if self.line_preview_cache.borrow().key != Some(key) {
            let mut cache = self.line_preview_cache.borrow_mut();
            cache.cells.clear();
            if let (Some(start), Some(end)) = (self.editor_start, self.editor_end) {
                let thickness = self.brush + 1;
                let half = thickness as i32 / 2;
                for (x, y) in raster::line_points(start, end) {
                    for py in y - half..y - half + thickness as i32 {
                        for px in x - half..x - half + thickness as i32 {
                            cache.cells.insert((px, py));
                        }
                    }
                }
            }
            cache.key = Some(key);
        }
        Some(Ref::map(self.line_preview_cache.borrow(), |cache| {
            &cache.cells
        }))
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
            EditorTool::Circle
            | EditorTool::Diamond
            | EditorTool::HollowCircle
            | EditorTool::HollowSquare
            | EditorTool::Plus
            | EditorTool::X => {
                let dx = x - start.0;
                let dy = y - start.1;
                let radius = start.0.abs_diff(end.0).max(start.1.abs_diff(end.1)) as i32;
                brush_offset_contains(
                    match self.tool {
                        EditorTool::Circle => BrushShape::Circle,
                        EditorTool::Diamond => BrushShape::Diamond,
                        EditorTool::HollowCircle => BrushShape::HollowCircle,
                        EditorTool::HollowSquare => BrushShape::HollowSquare,
                        EditorTool::Plus => BrushShape::Plus,
                        EditorTool::X => BrushShape::X,
                        _ => unreachable!(),
                    },
                    radius,
                    dx,
                    dy,
                )
            }
            EditorTool::Line => line_contains(start, end, self.brush + 1, x, y),
            EditorTool::Select => x == min_x || x == max_x || y == min_y || y == max_y,
        }
    }

    /// Click inside the popup selects that material; click outside closes it.
    fn click_picker(&mut self, col: u16, row: u16, world: &World) {
        let area = ui::picker_rect(
            world.width as u16,
            (world.height as u16) / 2 + ui::MAX_STATUS_ROWS,
        );
        if col > area.x
            && col < area.x + area.width.saturating_sub(1)
            && row > area.y
            && row < area.y + area.height.saturating_sub(1)
        {
            let visible = area.height.saturating_sub(2) as usize;
            let offset = ui::picker_scroll_offset(self.picker_cursor, area.height);
            let r = offset + (row - area.y).saturating_sub(1) as usize;
            if r < Material::ALL.len() && r < offset + visible {
                self.picker_cursor = r;
                self.selected = Material::ALL[r];
                self.picker_open = false;
                self.picker_query.clear();
            }
        } else {
            self.picker_open = false;
            self.picker_query.clear();
        }
    }

    /// Click inside the scene menu popup selects that row; click outside closes it.
    fn click_scene_menu(&mut self, col: u16, row: u16, world: &World) {
        if self.scene_menu.saving {
            return;
        }
        let popup = ui::scene_menu_rect(
            world.width as u16,
            (world.height as u16) / 2 + ui::MAX_STATUS_ROWS,
        );

        if col >= popup.x
            && col < popup.x + popup.width
            && row >= popup.y
            && row < popup.y + popup.height
        {
            let visible = popup.height.saturating_sub(4) as usize;
            let row_in_popup = row.saturating_sub(popup.y + 1) as usize;
            if col > popup.x
                && col < popup.x + popup.width.saturating_sub(1)
                && row > popup.y
                && row_in_popup < visible
            {
                let offset = ui::scene_scroll_offset(
                    self.scene_menu.cursor,
                    self.scene_menu.scenes.len(),
                    popup.height,
                );
                let index = offset + row_in_popup;
                if index < self.scene_menu.scenes.len() {
                    self.scene_menu.cursor = index;
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
    shape: BrushShape,
    mirror: Option<MirrorAxis>,
) {
    let r = radius as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            if brush_offset_contains(shape, r, dx, dy) {
                paint_cell(world, cx + dx, cy + dy, m, mirror);
            }
        }
    }
}

fn brush_offset_contains(shape: BrushShape, radius: i32, dx: i32, dy: i32) -> bool {
    match shape {
        BrushShape::Circle => dx * dx + dy * dy <= radius * radius,
        BrushShape::Square => dx.abs() <= radius && dy.abs() <= radius,
        BrushShape::Diamond => dx.abs() + dy.abs() <= radius,
        BrushShape::HollowCircle => {
            let d2 = dx * dx + dy * dy;
            let r2 = radius * radius;
            if radius == 0 {
                d2 == 0
            } else {
                d2 <= r2 && d2 > (radius - 1) * (radius - 1)
            }
        }
        BrushShape::HollowSquare => {
            let a = dx.abs();
            let b = dy.abs();
            a <= radius && b <= radius && (a == radius || b == radius)
        }
        BrushShape::Plus => dx.abs() <= radius && dy == 0 || dy.abs() <= radius && dx == 0,
        BrushShape::X => dx.abs() == dy.abs() && dx.abs() <= radius,
    }
}

fn paint_cell(world: &mut World, x: i32, y: i32, material: Material, mirror: Option<MirrorAxis>) {
    if x < 0 || y < 0 {
        return;
    }
    world.paint(x as usize, y as usize, material);
    if let Some((mx, my)) = mirror_point(world, x, y, mirror) {
        world.paint(mx, my, material);
    }
}

fn paint_state(
    world: &mut World,
    x: i32,
    y: i32,
    state: (Material, u16, u8, i16),
    mirror: Option<MirrorAxis>,
) {
    if x < 0 || y < 0 {
        return;
    }
    world.paint_state(x as usize, y as usize, state);
    if let Some((mx, my)) = mirror_point(world, x, y, mirror) {
        world.paint_state(mx, my, state);
    }
}

fn mirror_point(
    world: &World,
    x: i32,
    y: i32,
    mirror: Option<MirrorAxis>,
) -> Option<(usize, usize)> {
    let (mx, my) = match mirror? {
        MirrorAxis::Horizontal => (world.width as i32 - 1 - x, y),
        MirrorAxis::Vertical => (x, world.height as i32 - 1 - y),
    };
    (mx >= 0 && my >= 0).then_some((mx as usize, my as usize))
}

/// Whether a point is covered by an endpoint-inclusive rasterized line.
fn line_contains(start: (i32, i32), end: (i32, i32), thickness: usize, px: i32, py: i32) -> bool {
    let half = thickness as i32 / 2;
    raster::line_points(start, end).into_iter().any(|(x, y)| {
        (x - half..x - half + thickness as i32).contains(&px)
            && (y - half..y - half + thickness as i32).contains(&py)
    })
}

fn for_line_points(x0: i32, y0: i32, x1: i32, y1: i32, mut f: impl FnMut(i32, i32)) {
    for (x, y) in raster::line_points((x0, y0), (x1, y1)) {
        f(x, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn left_click(column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn picker_typeahead_jumps_to_matching_material() {
        let mut app = App {
            picker_open: true,
            picker_cursor: 0,
            ..App::default()
        };
        let mut world = World::new(4, 4);

        let type_char = |app: &mut App, world: &mut World, c: char| {
            app.handle(
                &Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
                world,
            );
        };

        type_char(&mut app, &mut world, 's');
        type_char(&mut app, &mut world, 'a');
        assert_eq!(Material::ALL[app.picker_cursor], Material::Sand);
        assert_eq!(app.picker_query, "sa");
        assert!(app.picker_open);

        type_char(&mut app, &mut world, 'l');
        // "sal" matches Salt (Sand no longer matches the longer prefix).
        assert_eq!(Material::ALL[app.picker_cursor], Material::Salt);

        app.picker_query.clear();
        type_char(&mut app, &mut world, 'g');
        type_char(&mut app, &mut world, 'l');
        type_char(&mut app, &mut world, 'a');
        assert_eq!(Material::ALL[app.picker_cursor], Material::Glass);

        app.picker_query.clear();
        type_char(&mut app, &mut world, 'c');
        type_char(&mut app, &mut world, '4');
        assert_eq!(Material::ALL[app.picker_cursor], Material::C4);
        assert_eq!(app.picker_query, "c4");
        assert!(app.picker_open);

        // Enter still confirms the selection.
        app.handle(
            &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            &mut world,
        );
        assert!(!app.picker_open);
        assert_eq!(app.selected, Material::C4);
        assert!(app.picker_query.is_empty());
    }

    #[test]
    fn confirmation_blocks_mouse_painting() {
        let mut app = App {
            confirm: Confirm::Clear,
            ..App::default()
        };
        let mut world = World::new(10, 10);

        app.handle(&left_click(2, 2), &mut world);

        assert_eq!(world.get(2, 4), Material::Empty);
        assert!(!app.drawing);
    }

    #[test]
    fn tool_picker_blocks_mouse_painting() {
        let mut app = App {
            tool_picker_open: true,
            ..App::default()
        };
        let mut world = World::new(10, 10);

        app.handle(&left_click(2, 2), &mut world);

        assert_eq!(world.get(2, 4), Material::Empty);
        assert!(!app.drawing);
    }

    #[test]
    fn scene_menu_border_click_does_not_select_a_scene() {
        let mut app = App::default();
        app.scene_menu.scenes = vec!["first".into(), "second".into()];
        app.scene_menu.cursor = 1;
        let world = World::new(10, 10);
        let popup = ui::scene_menu_rect(
            world.width as u16,
            (world.height as u16) / 2 + ui::MAX_STATUS_ROWS,
        );

        app.click_scene_menu(popup.x + 1, popup.y, &world);

        assert_eq!(app.scene_menu.cursor, 1);
    }

    #[test]
    fn compressed_undo_restores_exact_world_state() {
        let mut world = World::new(4, 3);
        world.paint_state(0, 0, (Material::Fire, 17, 23, 900));
        world.paint_state(3, 2, (Material::Ice, 5, 91, -20));
        world.set_velocity(0, 0, -3, 4);
        world.set_velocity(3, 2, 2, -1);
        world.paint(1, 1, Material::Sand);
        world.step();
        let expected_grid: Vec<u8> = world.grid().iter().map(|m| m.to_u8()).collect();
        let expected_life = world.life().to_vec();
        let expected_seed = world.seed().to_vec();
        let expected_vx = world.vx().to_vec();
        let expected_vy = world.vy().to_vec();
        let expected_vy_frac = world.vy_frac().to_vec();
        let expected_y_frac = world.y_frac().to_vec();
        assert!(expected_vy_frac.iter().any(|&value| value != 0));
        let expected_temp = world.temp().to_vec();
        let expected_vx_frac = world.vx_frac().to_vec();
        let expected_x_frac = world.x_frac().to_vec();
        let expected_air_mass = world.air_mass().to_vec();
        let expected_o2 = world.o2().to_vec();
        let expected_exhaust = world.exhaust().to_vec();
        let expected_fuel_vapor = world.fuel_vapor().to_vec();
        let snapshot = UndoState::from_world(&world);

        world.clear();
        snapshot.restore(&mut world);

        assert_eq!(
            world.grid().iter().map(|m| m.to_u8()).collect::<Vec<_>>(),
            expected_grid
        );
        assert_eq!(world.life(), expected_life);
        assert_eq!(world.seed(), expected_seed);
        assert_eq!(world.vx(), expected_vx);
        assert_eq!(world.vy(), expected_vy);
        assert_eq!(world.vy_frac(), expected_vy_frac);
        assert_eq!(world.y_frac(), expected_y_frac);
        assert_eq!(world.temp(), expected_temp);
        assert_eq!(world.vx_frac(), expected_vx_frac);
        assert_eq!(world.x_frac(), expected_x_frac);
        assert_eq!(world.air_mass(), expected_air_mass);
        assert_eq!(world.o2(), expected_o2);
        assert_eq!(world.exhaust(), expected_exhaust);
        assert_eq!(world.fuel_vapor(), expected_fuel_vapor);
    }

    #[test]
    fn line_preview_cache_invalidates_when_endpoint_changes() {
        let mut app = App {
            tool: EditorTool::Line,
            editor_start: Some((1, 1)),
            editor_end: Some((3, 1)),
            brush: 0,
            ..App::default()
        };
        assert!(app.line_preview_cells().unwrap().contains(&(3, 1)));

        app.editor_end = Some((1, 3));
        let cells = app.line_preview_cells().unwrap();
        assert!(cells.contains(&(1, 3)));
        assert!(!cells.contains(&(3, 1)));
    }
}
