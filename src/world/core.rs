use super::scenes::{seed_house, seed_skyscraper};
use super::*;
use crate::scene_manager::SceneState;

/// A loadable scene preset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    House,
    Skyscraper,
}

impl Scene {
    pub const ALL: [Scene; 2] = [Scene::House, Scene::Skyscraper];

    pub fn name(self) -> &'static str {
        match self {
            Scene::House => "House Cross-Section",
            Scene::Skyscraper => "Skyscraper Cross-Section",
        }
    }
}

pub struct World {
    pub width: usize,
    pub height: usize,
    pub(super) grid: Vec<Material>,
    pub(super) life: Vec<u16>,
    pub(super) seed: Vec<u8>,
    pub(super) vx: Vec<i8>,
    pub(super) vy: Vec<i8>,
    /// Fractional vertical velocity and displacement in quarter-cell units.
    pub(super) vy_frac: Vec<i8>,
    pub(super) y_frac: Vec<i8>,
    /// Approximate Celsius temperature per cell.
    pub(super) temp: Vec<i16>,
    /// Scratch buffer for heat diffusion.
    pub(super) temp_next: Vec<i16>,
    pub(super) moved_tick: Vec<u64>,
    pub(super) active_chunks: Vec<bool>,
    pub(super) next_active_chunks: Vec<bool>,
    /// Generation tags for connected-component structural physics.
    structural_seen: Vec<u32>,
    structural_generation: u32,
    structural_stack: Vec<usize>,
    structural_component: Vec<usize>,
    structural_ordered: Vec<usize>,
    structural_membership: Vec<bool>,
    structural_targets: Vec<bool>,
    structural_moves: Vec<(usize, usize, u16, u8, i16, i8, i8, i8, i8)>,
    structural_displaced: Vec<(Material, u16, u8, i16, i8, i8, i8, i8)>,
    pub(super) chunks_x: usize,
    pub(super) chunks_y: usize,
    pub(super) tick: u64,
    pub(super) scene: Scene,
}

impl World {
    pub fn new(width: usize, height: usize) -> Self {
        let n = width * height;
        Self {
            width,
            height,
            grid: vec![Empty; n],
            life: vec![0; n],
            seed: vec![0; n],
            vx: vec![0; n],
            vy: vec![0; n],
            vy_frac: vec![0; n],
            y_frac: vec![0; n],
            temp: vec![AMBIENT_TEMP; n],
            temp_next: vec![AMBIENT_TEMP; n],
            moved_tick: vec![u64::MAX; n],
            active_chunks: vec![false; chunks_len(width, height)],
            next_active_chunks: vec![false; chunks_len(width, height)],
            structural_seen: vec![0; n],
            structural_generation: 0,
            structural_stack: Vec::new(),
            structural_component: Vec::new(),
            structural_ordered: Vec::new(),
            structural_membership: vec![false; n],
            structural_targets: vec![false; n],
            structural_moves: Vec::new(),
            structural_displaced: Vec::new(),
            chunks_x: chunks_x(width),
            chunks_y: chunks_y(height),
            tick: 0,
            scene: Scene::House,
        }
    }

    pub(super) fn idx(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn get(&self, x: usize, y: usize) -> Material {
        self.grid[self.idx(x, y)]
    }
    pub fn seed_at(&self, x: usize, y: usize) -> u8 {
        self.seed[self.idx(x, y)]
    }
    pub fn life_at(&self, x: usize, y: usize) -> u16 {
        self.life[self.idx(x, y)]
    }
    /// Return the state needed to render one in-bounds cell with one index calculation.
    pub fn render_state(&self, x: usize, y: usize) -> (Material, u8, u16) {
        let i = self.idx(x, y);
        (self.grid[i], self.seed[i], self.life[i])
    }
    pub fn temp_at(&self, x: usize, y: usize) -> i16 {
        self.temp[self.idx(x, y)]
    }
    pub fn velocity_at(&self, x: usize, y: usize) -> (i8, i8) {
        let i = self.idx(x, y);
        (self.vx[i], self.vy[i])
    }
    pub(crate) fn set_velocity(&mut self, x: usize, y: usize, vx: i8, vy: i8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = self.idx(x, y);
        self.vx[i] = vx.clamp(-MAX_VELOCITY, MAX_VELOCITY);
        self.vy[i] = vy.clamp(-MAX_VELOCITY, MAX_VELOCITY);
        self.vy_frac[i] = 0;
        self.y_frac[i] = 0;
    }

    /// Expose vy_frac for serialization and snapshots.
    pub fn vy_frac(&self) -> &[i8] {
        &self.vy_frac
    }

    /// Expose y_frac for serialization and snapshots.
    pub fn y_frac(&self) -> &[i8] {
        &self.y_frac
    }

    /// Paint a single cell (used by the brush; does not touch movement bookkeeping).
    pub fn paint(&mut self, x: usize, y: usize, m: Material) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = self.idx(x, y);
        self.grid[i] = m;
        self.seed[i] = rand::random();
        self.life[i] = rand_life(m);
        self.vx[i] = 0;
        self.vy[i] = 0;
        self.vy_frac[i] = 0;
        self.y_frac[i] = 0;
        self.temp[i] = m.painted_temperature();
        self.activate_now(x, y);
    }
    /// Return a cell including its visual, lifetime, and temperature state for editor copy/paste.
    pub fn cell_state(&self, x: usize, y: usize) -> Option<(Material, u16, u8, i16)> {
        (x < self.width && y < self.height).then(|| {
            let i = self.idx(x, y);
            (self.grid[i], self.life[i], self.seed[i], self.temp[i])
        })
    }

    /// Restore a cell copied by the editor without regenerating its state.
    pub fn paint_state(&mut self, x: usize, y: usize, state: (Material, u16, u8, i16)) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = self.idx(x, y);
        self.grid[i] = state.0;
        self.life[i] = state.1;
        self.seed[i] = state.2;
        self.vx[i] = 0;
        self.vy[i] = 0;
        self.vy_frac[i] = 0;
        self.y_frac[i] = 0;
        self.temp[i] = state.3;
        self.activate_now(x, y);
    }

    pub fn clear(&mut self) {
        for i in 0..self.grid.len() {
            self.grid[i] = Empty;
            self.life[i] = 0;
            self.seed[i] = rand::random();
            self.temp[i] = AMBIENT_TEMP;
        }
        self.vx.fill(0);
        self.vy.fill(0);
        self.vy_frac.fill(0);
        self.y_frac.fill(0);
        self.tick = 0;
        self.moved_tick.fill(u64::MAX);
        self.active_chunks.fill(false);
        self.next_active_chunks.fill(false);
        self.structural_seen.fill(0);
        self.structural_generation = 0;
        self.structural_stack.clear();
        self.structural_component.clear();
        self.structural_ordered.clear();
        self.structural_membership.fill(false);
        self.structural_targets.fill(false);
        self.structural_moves.clear();
        self.structural_displaced.clear();
    }

    /// Expose the grid for serialization.
    pub fn grid(&self) -> &[Material] {
        &self.grid
    }

    /// Expose life values for serialization.
    pub fn life(&self) -> &[u16] {
        &self.life
    }

    /// Expose seed values for serialization.
    pub fn seed(&self) -> &[u8] {
        &self.seed
    }

    /// Expose temperatures for serialization.
    pub fn temp(&self) -> &[i16] {
        &self.temp
    }

    /// Expose horizontal velocities for serialization and snapshots.
    pub fn vx(&self) -> &[i8] {
        &self.vx
    }

    /// Expose vertical velocities for serialization and snapshots.
    pub fn vy(&self) -> &[i8] {
        &self.vy
    }

    pub fn load_scene(&mut self, scene: Scene) {
        self.clear();
        self.scene = scene;
        match scene {
            Scene::House => seed_house(self),
            Scene::Skyscraper => seed_skyscraper(self),
        }
    }

    /// Restore a previously saved scene state, clipping or padding as needed if
    /// the terminal size differs from the saved scene size.
    pub fn restore_from(&mut self, state: &SceneState) {
        self.clear();

        let cw = self.width.min(state.width);
        let ch = self.height.min(state.height);
        for y in 0..ch {
            for x in 0..cw {
                let src = y * state.width + x;
                let dst = self.idx(x, y);
                self.grid[dst] = state
                    .grid
                    .get(src)
                    .and_then(|&m| Material::from_u8(m))
                    .unwrap_or(Empty);
                self.life[dst] = state.life.get(src).copied().unwrap_or(0);
                self.seed[dst] = state.seed.get(src).copied().unwrap_or(0);
                self.set_velocity(
                    x,
                    y,
                    state.vx.get(src).copied().unwrap_or(0),
                    state.vy.get(src).copied().unwrap_or(0),
                );
                self.vy_frac[dst] = state
                    .vy_frac
                    .get(src)
                    .copied()
                    .unwrap_or(0)
                    .clamp(-(VELOCITY_SCALE - 1), VELOCITY_SCALE - 1);
                self.y_frac[dst] = state
                    .y_frac
                    .get(src)
                    .copied()
                    .unwrap_or(0)
                    .clamp(-(VELOCITY_SCALE - 1), VELOCITY_SCALE - 1);
                self.temp[dst] = state
                    .temp
                    .get(src)
                    .copied()
                    .unwrap_or_else(|| self.grid[dst].painted_temperature());
            }
        }
        self.tick = 0;
        self.moved_tick.fill(u64::MAX);
        self.active_chunks.fill(true);
        self.next_active_chunks.fill(false);
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        let mut grid = vec![Empty; width * height];
        let mut life = vec![0; width * height];
        let mut seed = vec![0; width * height];
        let mut vx = vec![0; width * height];
        let mut vy = vec![0; width * height];
        let mut vy_frac = vec![0; width * height];
        let mut y_frac = vec![0; width * height];
        let mut temp = vec![AMBIENT_TEMP; width * height];
        let cw = width.min(self.width);
        let ch = height.min(self.height);
        for y in 0..ch {
            for x in 0..cw {
                grid[y * width + x] = self.grid[y * self.width + x];
                life[y * width + x] = self.life[y * self.width + x];
                seed[y * width + x] = self.seed[y * self.width + x];
                let (cell_vx, cell_vy) = self.velocity_at(x, y);
                vx[y * width + x] = cell_vx;
                vy[y * width + x] = cell_vy;
                vy_frac[y * width + x] = self.vy_frac[y * self.width + x];
                y_frac[y * width + x] = self.y_frac[y * self.width + x];
                temp[y * width + x] = self.temp[y * self.width + x];
            }
        }
        self.width = width;
        self.height = height;
        self.grid = grid;
        self.life = life;
        self.seed = seed;
        self.vx = vx;
        self.vy = vy;
        self.vy_frac = vy_frac;
        self.y_frac = y_frac;
        self.temp = temp;
        self.temp_next = vec![AMBIENT_TEMP; width * height];
        self.moved_tick = vec![u64::MAX; width * height];
        self.chunks_x = chunks_x(width);
        self.chunks_y = chunks_y(height);
        self.active_chunks = vec![true; chunks_len(width, height)];
        self.next_active_chunks = vec![false; chunks_len(width, height)];
        self.structural_seen = vec![0; width * height];
        self.structural_generation = 0;
        self.structural_stack.clear();
        self.structural_component.clear();
        self.structural_ordered.clear();
        self.structural_membership = vec![false; width * height];
        self.structural_targets = vec![false; width * height];
        self.structural_moves.clear();
        self.structural_displaced.clear();
    }

    // --- internal mutation helpers (used only during a step) ---

    pub(super) fn activate_idx(&mut self, i: usize) {
        let x = i % self.width;
        let y = i / self.width;
        self.activate_next(x, y);
    }

    pub(super) fn activate_now(&mut self, x: usize, y: usize) {
        activate_chunk_neighborhood(
            self.width,
            self.height,
            self.chunks_x,
            x,
            y,
            &mut self.active_chunks,
        );
    }

    pub(super) fn activate_next(&mut self, x: usize, y: usize) {
        activate_chunk_neighborhood(
            self.width,
            self.height,
            self.chunks_x,
            x,
            y,
            &mut self.next_active_chunks,
        );
    }

    pub(super) fn put(&mut self, i: usize, m: Material, life: u16) {
        let prev_temp = self.temp[i];
        self.grid[i] = m;
        self.life[i] = life;
        self.seed[i] = rand::random();
        self.vx[i] = 0;
        self.vy[i] = 0;
        self.vy_frac[i] = 0;
        self.y_frac[i] = 0;
        // Heat sources clamp; everything else keeps the cell's thermal history so
        // phase changes (ice→water, lava→stone) stay continuous instead of
        // snapping back to ambient.
        self.temp[i] = m.heat_source_temp().unwrap_or(prev_temp);
        self.moved_tick[i] = self.tick;
        self.activate_idx(i);
    }

    pub(super) fn swap(&mut self, a: usize, b: usize) {
        self.grid.swap(a, b);
        self.life.swap(a, b);
        self.seed.swap(a, b);
        self.vx.swap(a, b);
        self.vy.swap(a, b);
        self.vy_frac.swap(a, b);
        self.y_frac.swap(a, b);
        self.temp.swap(a, b);
        self.moved_tick[a] = self.tick;
        self.moved_tick[b] = self.tick;
        self.activate_idx(a);
        self.activate_idx(b);
    }

    pub(super) fn adj(&self, x: usize, y: usize, dx: i32, dy: i32) -> Option<(usize, usize)> {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx >= 0 && ny >= 0 && (nx as usize) < self.width && (ny as usize) < self.height {
            Some((nx as usize, ny as usize))
        } else {
            None
        }
    }

    /// Try to move the cell at `(x, y)` one step in `(dx, dy)` if `allow`
    /// accepts the target. Returns true if it moved.
    pub(super) fn try_step(
        &mut self,
        x: usize,
        y: usize,
        dx: i32,
        dy: i32,
        allow: impl Fn(Material) -> bool,
    ) -> bool {
        self.adj(x, y, dx, dy)
            .is_some_and(|(tx, ty)| self.try_into(x, y, tx, ty, allow))
    }

    pub(super) fn try_into(
        &mut self,
        x: usize,
        y: usize,
        tx: usize,
        ty: usize,
        allow: impl Fn(Material) -> bool,
    ) -> bool {
        let ti = self.idx(tx, ty);
        if allow(self.grid[ti]) {
            self.swap(self.idx(x, y), ti);
            true
        } else {
            false
        }
    }

    /// Trace the cell at `(x, y)` along its stored velocity vector (clamped
    /// vx/vy) using integer DDA (Bresenham-style) so the line passes through
    /// each intermediate cell with no tunneling. Returns true if the cell
    /// moved at least one step.
    ///
    /// Effective vertical displacement each tick is computed by accumulating
    /// `y_frac + vy_frac` into a carry and remainder, using `dy = vy + carry`.
    /// Fractional sub-cell state is carried through swaps and cleared on
    /// vertical collision / OOB.
    ///
    /// * Pure horizontal steps require `target == Empty`.
    /// * Downward steps (dy > 0) use density-based displacement
    ///   (`material.can_sink_into`).
    /// * Upward steps (dy < 0) require Empty or a lighter fluid.
    /// * Diagonal advances are split into cardinal steps so occupied corners
    ///   cannot be skipped.
    ///
    /// When a component is blocked, traversal stops after zeroing that velocity
    /// component on the moving cell. The caller should proceed to legacy
    /// movement when this returns false, or return early when it returns true.
    pub(super) fn try_velocity_move(&mut self, x: usize, y: usize) -> bool {
        let i = self.idx(x, y);
        let vx = self.vx[i];
        let vy = self.vy[i];
        let vy_frac = self.vy_frac[i];
        let y_frac = self.y_frac[i];

        // Compute effective vertical integer displacement.
        let sum = (y_frac as i16) + (vy_frac as i16);
        let carry = sum / (VELOCITY_SCALE as i16);
        let new_y_frac = sum % (VELOCITY_SCALE as i16);
        let dy = (vy as i16 + carry) as i8;

        // Write back updated y_frac before any movement.
        self.y_frac[i] = new_y_frac as i8;

        if vx == 0 && dy == 0 {
            // No integer movement this tick, but if sub-cell motion exists,
            // keep the cell active so acceleration (gravity) continues.
            if vy_frac != 0 || new_y_frac != 0 || vy != 0 {
                self.activate_idx(i);
            }
            return false;
        }

        let material = self.grid[i];
        // Only non-structural movable materials use velocity-driven movement.
        if !material.is_fluid() {
            return false;
        }

        let sx = vx.signum() as i32;
        let sy = dy.signum() as i32;
        let adx = vx.unsigned_abs() as i32;
        let ady = dy.unsigned_abs() as i32;
        let steps = adx.max(ady);

        let mut cx = x as i32;
        let mut cy = y as i32;
        let mut nom = 0i32;
        let mut moved = false;

        for _ in 0..steps {
            let step_x: bool;
            let step_y: bool;

            if adx >= ady {
                step_x = true;
                nom += ady;
                step_y = nom >= adx;
                if step_y {
                    nom -= adx;
                }
            } else {
                step_y = true;
                nom += adx;
                step_x = nom >= ady;
                if step_x {
                    nom -= ady;
                }
            }

            let mut blocked = false;

            // Split diagonal advances into cardinal steps. This checks both cells
            // touched at a corner instead of tunneling between them.
            if step_x {
                let nx = cx + sx;
                let ci = self.idx(cx as usize, cy as usize);
                if nx < 0 || nx >= self.width as i32 {
                    self.vx[ci] = 0;
                    blocked = true;
                } else {
                    let ni = self.idx(nx as usize, cy as usize);
                    if self.grid[ni].is_empty() {
                        self.swap(ci, ni);
                        cx = nx;
                        moved = true;
                    } else {
                        self.vx[ci] = 0;
                        blocked = true;
                    }
                }
            }

            if step_y {
                let ny = cy + sy;
                let ci = self.idx(cx as usize, cy as usize);
                if ny < 0 || ny >= self.height as i32 {
                    self.vy[ci] = 0;
                    self.vy_frac[ci] = 0;
                    self.y_frac[ci] = 0;
                    blocked = true;
                } else {
                    let ni = self.idx(cx as usize, ny as usize);
                    let target = self.grid[ni];
                    let allowed = if sy > 0 {
                        material.can_sink_into(target)
                    } else {
                        target.is_empty()
                            || (target.is_fluid() && material.density() < target.density())
                    };
                    if allowed {
                        self.swap(ci, ni);
                        cy = ny;
                        moved = true;
                    } else {
                        self.vy[ci] = 0;
                        self.vy_frac[ci] = 0;
                        self.y_frac[ci] = 0;
                        blocked = true;
                    }
                }
            }

            if blocked {
                break;
            }
        }

        moved
    }

    /// Whether the current fixed-point velocity produces a whole vertical step.
    pub(super) fn has_vertical_step(&self, i: usize) -> bool {
        self.vy[i] != 0
            || (self.y_frac[i] as i16 + self.vy_frac[i] as i16).unsigned_abs()
                >= VELOCITY_SCALE as u16
    }

    /// Apply the appropriate vertical force to the cell at `i` based on its
    /// material classification. Gravity accelerates powders, liquids, embers,
    /// and sparks downward; buoyancy lifts gases and fire upward.
    ///
    /// Adds ±GRAVITY_PER_TICK to vy_frac each force tick, carrying to vy when
    /// vy_frac crosses ±4. Fixed combined velocity = vy*4 + vy_frac, clamped
    /// to ±MAX_VELOCITY*4.
    pub(super) fn apply_vertical_force(&mut self, i: usize) {
        let material = self.grid[i];
        let force = if matches!(material, Sand | BrokenGlass | Ash | Salt | Gunpowder | Coal)
            || material.is_liquid()
            || matches!(material, Ember | FireworkSpark)
        {
            Some(GRAVITY_PER_TICK) // gravity: positive = down
        } else if matches!(material, Fire | Steam | Smoke) {
            Some(-GRAVITY_PER_TICK) // buoyancy: negative = up
        } else {
            None
        };
        let Some(f) = force else { return };

        // Combine vy and vy_frac into fixed velocity, add force, clamp, and split back.
        let fixed = (self.vy[i] as i16) * (VELOCITY_SCALE as i16) + (self.vy_frac[i] as i16);
        let max_fixed = (MAX_VELOCITY as i16) * (VELOCITY_SCALE as i16);
        let new_fixed = (fixed + f as i16).clamp(-max_fixed, max_fixed);
        self.vy[i] = (new_fixed / VELOCITY_SCALE as i16) as i8;
        self.vy_frac[i] = (new_fixed % VELOCITY_SCALE as i16) as i8;
    }

    /// Score horizontal room in one direction, preferring routes that lead to
    /// a downward opening. This approximates local liquid pressure without a
    /// separate pressure grid.
    pub(super) fn flow_score(&self, x: usize, y: usize, dir: i32, range: usize) -> usize {
        let mut cx = x;
        for distance in 1..=range {
            let Some((nx, _)) = self.adj(cx, y, dir, 0) else {
                break;
            };
            if self.get(nx, y) != Empty {
                break;
            }
            if self
                .adj(nx, y, 0, 1)
                .is_some_and(|(bx, by)| self.get(bx, by) == Empty)
            {
                return range + 1 - distance;
            }
            cx = nx;
        }
        0
    }

    pub(super) fn noise(&self, x: usize, y: usize, salt: u32) -> u32 {
        let i = self.idx(x, y);
        let mut n = (x as u32).wrapping_mul(0x9E37_79B9)
            ^ (y as u32).wrapping_mul(0x85EB_CA6B)
            ^ (self.tick as u32).wrapping_mul(0xC2B2_AE35)
            ^ (self.seed[i] as u32).wrapping_mul(0x27D4_EB2D)
            ^ salt;
        n ^= n >> 16;
        n = n.wrapping_mul(0x7FEB_352D);
        n ^= n >> 15;
        n = n.wrapping_mul(0x846C_A68B);
        n ^ (n >> 16)
    }

    pub(super) fn dirs(&self, x: usize, y: usize, salt: u32) -> (i32, i32) {
        if self.noise(x, y, salt) & 1 == 0 {
            (-1, 1)
        } else {
            (1, -1)
        }
    }

    pub(super) fn chance(&self, x: usize, y: usize, salt: u32, per_mille: u32) -> bool {
        self.noise(x, y, salt) % 1000 < per_mille
    }

    pub(super) fn chance_idx(&self, i: usize, salt: u32, per_mille: u32) -> bool {
        self.chance(i % self.width, i / self.width, salt, per_mille)
    }

    pub(super) fn roll(&self, x: usize, y: usize, salt: u32) -> u32 {
        self.noise(x, y, salt) % 1000
    }

    pub(super) fn n4(&self, x: usize, y: usize) -> [Option<(usize, usize)>; 4] {
        let mut out = [None; 4];
        let dirs = [(0, -1), (0, 1), (-1, 0), (1, 0)];
        let mut k = 0;
        for (dx, dy) in dirs {
            if let Some(p) = self.adj(x, y, dx, dy) {
                out[k] = Some(p);
                k += 1;
            }
        }
        out
    }

    pub(super) fn n8(&self, x: usize, y: usize) -> [Option<(usize, usize)>; 8] {
        let mut out = [None; 8];
        let mut k = 0;
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                if let Some(p) = self.adj(x, y, dx, dy) {
                    out[k] = Some(p);
                    k += 1;
                }
            }
        }
        out
    }

    pub(super) fn structural_can_displace(&self, material: Material, other: Material) -> bool {
        other.is_empty()
            || other.is_gas()
            || (other.is_fluid() && material.density() > other.density())
    }

    pub(super) fn component_can_translate(
        &self,
        material: Material,
        component: &[usize],
        in_component: &[bool],
        dx: i32,
        dy: i32,
    ) -> bool {
        component.iter().all(|&i| {
            let x = i % self.width;
            let y = i / self.width;
            let Some((tx, ty)) = self.adj(x, y, dx, dy) else {
                return false;
            };
            let ti = self.idx(tx, ty);
            in_component[ti] || self.structural_can_displace(material, self.grid[ti])
        })
    }

    pub(super) fn translate_structural_component(
        &mut self,
        material: Material,
        component: &[usize],
        in_component: &[bool],
        dx: i32,
        dy: i32,
    ) {
        let mut ordered = std::mem::take(&mut self.structural_ordered);
        ordered.clear();
        ordered.extend_from_slice(component);
        ordered.sort_unstable_by(|&a, &b| {
            let ax = a % self.width;
            let ay = a / self.width;
            let bx = b % self.width;
            let by = b / self.width;
            match dy.cmp(&0) {
                std::cmp::Ordering::Greater => by.cmp(&ay),
                std::cmp::Ordering::Less => ay.cmp(&by),
                std::cmp::Ordering::Equal => match dx.cmp(&0) {
                    std::cmp::Ordering::Greater => bx.cmp(&ax),
                    std::cmp::Ordering::Less => ax.cmp(&bx),
                    std::cmp::Ordering::Equal => std::cmp::Ordering::Equal,
                },
            }
        });

        let mut target_set = std::mem::take(&mut self.structural_targets);
        target_set.fill(false);
        let mut moves = std::mem::take(&mut self.structural_moves);
        moves.clear();
        let mut displaced = std::mem::take(&mut self.structural_displaced);
        displaced.clear();
        for &i in &ordered {
            let x = i % self.width;
            let y = i / self.width;
            let (tx, ty) = self
                .adj(x, y, dx, dy)
                .expect("prechecked structural translation");
            let ti = self.idx(tx, ty);
            target_set[ti] = true;
            moves.push((
                i,
                ti,
                self.life[i],
                self.seed[i],
                self.temp[i],
                self.vx[i],
                self.vy[i],
                self.vy_frac[i],
                self.y_frac[i],
            ));
            if !in_component[ti] && self.grid[ti] != Empty {
                displaced.push((
                    self.grid[ti],
                    self.life[ti],
                    self.seed[ti],
                    self.temp[ti],
                    self.vx[ti],
                    self.vy[ti],
                    self.vy_frac[ti],
                    self.y_frac[ti],
                ));
            }
        }

        for &(i, _, _, _, _, _, _, _, _) in &moves {
            self.grid[i] = Empty;
            self.life[i] = 0;
            self.vx[i] = 0;
            self.vy[i] = 0;
            self.vy_frac[i] = 0;
            self.y_frac[i] = 0;
            self.temp[i] = AMBIENT_TEMP;
            self.moved_tick[i] = self.tick;
            self.activate_idx(i);
        }
        for &(_, ti, life, seed, temp, vx, vy, vy_frac, y_frac) in &moves {
            self.grid[ti] = material;
            self.life[ti] = life;
            self.seed[ti] = seed;
            self.temp[ti] = temp;
            self.vx[ti] = vx;
            self.vy[ti] = vy;
            self.vy_frac[ti] = vy_frac;
            self.y_frac[ti] = y_frac;
            self.moved_tick[ti] = self.tick;
            self.activate_idx(ti);
        }
        for &(i, _, _, _, _, _, _, _, _) in &moves {
            if target_set[i] {
                continue;
            }
            if let Some((m, life, seed, temp, vx, vy, vy_frac, y_frac)) = displaced.pop() {
                self.grid[i] = m;
                self.life[i] = life;
                self.seed[i] = seed;
                self.temp[i] = temp;
                self.vx[i] = vx;
                self.vy[i] = vy;
                self.vy_frac[i] = vy_frac;
                self.y_frac[i] = y_frac;
            }
        }

        ordered.clear();
        target_set.fill(false);
        moves.clear();
        displaced.clear();
        self.structural_ordered = ordered;
        self.structural_targets = target_set;
        self.structural_moves = moves;
        self.structural_displaced = displaced;
    }

    pub(super) fn try_translate_structural_component(
        &mut self,
        material: Material,
        component: &[usize],
        in_component: &[bool],
        dx: i32,
        dy: i32,
    ) -> bool {
        if !self.component_can_translate(material, component, in_component, dx, dy) {
            return false;
        }
        self.translate_structural_component(material, component, in_component, dx, dy);
        true
    }

    fn shatter_glass_component(&mut self, component: &[usize]) {
        for &i in component {
            self.grid[i] = BrokenGlass;
            self.life[i] = 0;
            self.vx[i] = 0;
            self.vy[i] = 0;
            self.vy_frac[i] = 0;
            self.y_frac[i] = 0;
            self.moved_tick[i] = self.tick;
            self.activate_idx(i);
        }
    }

    /// Move unsupported connected structural islands down one cell. Glass turns
    /// into broken glass when an island that fell on the previous tick hits support.
    pub(super) fn step_structural_components(&mut self) {
        let mut stack = std::mem::take(&mut self.structural_stack);
        stack.clear();
        let mut component = std::mem::take(&mut self.structural_component);
        component.clear();
        let mut in_component = std::mem::take(&mut self.structural_membership);
        in_component.fill(false);

        for material in [Wood, Stone, Glass] {
            self.structural_generation = self.structural_generation.wrapping_add(1);
            if self.structural_generation == 0 {
                self.structural_seen.fill(0);
                self.structural_generation = 1;
            }
            let generation = self.structural_generation;
            for y in (0..self.height).rev() {
                for x in 0..self.width {
                    let start = self.idx(x, y);
                    if self.grid[start] != material
                        || self.moved_tick[start] == self.tick
                        || self.structural_seen[start] == generation
                    {
                        continue;
                    }

                    stack.clear();
                    component.clear();
                    stack.push(start);
                    self.structural_seen[start] = generation;
                    while let Some(i) = stack.pop() {
                        component.push(i);
                        let cx = i % self.width;
                        let cy = i / self.width;
                        for n in self.n4(cx, cy) {
                            let Some((nx, ny)) = n else { continue };
                            let ni = self.idx(nx, ny);
                            if self.grid[ni] == material
                                && self.moved_tick[ni] != self.tick
                                && self.structural_seen[ni] != generation
                            {
                                self.structural_seen[ni] = generation;
                                stack.push(ni);
                            }
                        }
                    }

                    for &i in &component {
                        in_component[i] = true;
                    }
                    let unsupported = component.iter().all(|&i| {
                        let y = i / self.width;
                        y + 1 < self.height && {
                            let below = i + self.width;
                            in_component[below]
                                || self.structural_can_displace(material, self.grid[below])
                        }
                    });

                    if unsupported {
                        let _ = self.try_translate_structural_component(
                            material,
                            &component,
                            &in_component,
                            0,
                            1,
                        );
                    } else if material == Glass
                        && component.iter().any(|&i| {
                            // u64::MAX is the never-moved sentinel; at tick 0 it
                            // collides with wrapping_sub(1), so exclude it.
                            let mt = self.moved_tick[i];
                            mt != u64::MAX && mt == self.tick.wrapping_sub(1)
                        })
                    {
                        self.shatter_glass_component(&component);
                    }
                    for &i in &component {
                        in_component[i] = false;
                    }
                }
            }
        }

        stack.clear();
        component.clear();
        in_component.fill(false);
        self.structural_stack = stack;
        self.structural_component = component;
        self.structural_membership = in_component;
    }

    // --- the step ---

    pub fn step(&mut self) {
        self.next_active_chunks.fill(false);
        self.step_heat();
        self.step_structural_components();

        for chunk_y in (0..self.chunks_y).rev() {
            let chunks_ltr = self.tick.wrapping_add(chunk_y as u64).is_multiple_of(2);
            for chunk_k in 0..self.chunks_x {
                let chunk_x = if chunks_ltr {
                    chunk_k
                } else {
                    self.chunks_x - 1 - chunk_k
                };
                let chunk_i = chunk_y * self.chunks_x + chunk_x;
                if !self.active_chunks.get(chunk_i).copied().unwrap_or(false) {
                    continue;
                }

                let y0 = chunk_y * CHUNK_H;
                let y1 = ((chunk_y + 1) * CHUNK_H).min(self.height);
                let x0 = chunk_x * CHUNK_W;
                let x1 = ((chunk_x + 1) * CHUNK_W).min(self.width);

                for y in (y0..y1).rev() {
                    let ltr = self.tick.wrapping_add(y as u64).is_multiple_of(2);
                    for k in 0..(x1 - x0) {
                        let x = if ltr { x0 + k } else { x1 - 1 - k };
                        let i = self.idx(x, y);
                        if self.moved_tick[i] == self.tick {
                            continue;
                        }
                        let material = self.grid[i];
                        if material == Empty {
                            continue;
                        }
                        if material.flammable() {
                            let effective_temp = self.effective_temp(x, y);
                            self.activate_next(x, y);
                            if self.step_combustible(x, y, effective_temp) {
                                continue;
                            }
                        } else if material != Ice
                            && let Some((melt_temp, _, _)) = material.melt()
                        {
                            let effective_temp = self.effective_temp(x, y);
                            if effective_temp.max(0) as u16 >= melt_temp / 2 {
                                // Only track heat-soak when the cell is already warm enough
                                // that melting is plausible (avoids keeping cold sand active).
                                self.activate_next(x, y);
                                if self.step_melt(x, y, effective_temp) {
                                    continue;
                                }
                            }
                        }
                        match self.grid[i] {
                            Empty | Stone | Wood | Glass | Metal | Concrete => continue,
                            Fire => {
                                self.activate_next(x, y);
                                self.step_fire(x, y);
                            }
                            Ember => {
                                self.activate_next(x, y);
                                self.step_ember(x, y);
                            }
                            Steam | Smoke => {
                                self.activate_next(x, y);
                                self.step_gas(x, y);
                            }
                            Sand | BrokenGlass | Ash | Salt | Gunpowder | Coal => {
                                self.step_powder(x, y)
                            }
                            Water | Oil | Napalm | LiquidNitrogen | Mercury => {
                                self.step_liquid(x, y)
                            }
                            Acid | Lava => {
                                self.activate_next(x, y);
                                self.step_liquid(x, y);
                            }
                            Ice => {
                                self.activate_next(x, y);
                                self.step_ice(x, y);
                            }
                            Plant => {
                                self.activate_next(x, y);
                                self.step_plant(x, y);
                            }
                            Fuse => self.step_fuse(x, y),
                            Tnt => self.step_tnt(x, y),
                            C4 => self.step_c4(x, y),
                            Firework => self.step_firework(x, y),
                            FireworkSpark => {
                                self.activate_next(x, y);
                                self.step_firework_spark(x, y);
                            }
                            Faucet => {
                                self.activate_next(x, y);
                                self.step_faucet(x, y);
                            }
                            Drain => {
                                self.activate_next(x, y);
                                self.step_drain(x, y);
                            }
                        }
                    }
                }
            }
        }
        std::mem::swap(&mut self.active_chunks, &mut self.next_active_chunks);
        self.tick = self.tick.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocity_is_bounded_and_follows_generic_movement() {
        let mut world = World::new(3, 2);
        world.paint(0, 0, Sand);
        world.set_velocity(0, 0, -9, 12);
        assert_eq!(world.velocity_at(0, 0), (-4, 4));

        let source = world.idx(0, 0);
        let target = world.idx(1, 0);
        world.swap(source, target);
        assert_eq!(world.velocity_at(0, 0), (0, 0));
        assert_eq!(world.velocity_at(1, 0), (-4, 4));

        world.put(target, Smoke, 10);
        assert_eq!(world.velocity_at(1, 0), (0, 0));

        world.set_velocity(1, 0, 2, -2);
        world.paint(1, 0, Water);
        assert_eq!(world.velocity_at(1, 0), (0, 0));

        world.set_velocity(1, 0, 2, -2);
        world.paint_state(1, 0, (Ice, 7, 9, -20));
        assert_eq!(world.velocity_at(1, 0), (0, 0));
    }

    #[test]
    fn resize_preserves_velocity_and_clear_resets_it() {
        let mut world = World::new(2, 2);
        world.paint(1, 1, Water);
        world.set_velocity(1, 1, 3, -2);
        let i = world.idx(1, 1);
        world.vy_frac[i] = -3;
        world.y_frac[i] = 2;

        world.resize(4, 3);
        let i = world.idx(1, 1);
        assert_eq!(world.velocity_at(1, 1), (3, -2));
        assert_eq!((world.vy_frac[i], world.y_frac[i]), (-3, 2));
        assert_eq!(world.velocity_at(3, 2), (0, 0));

        world.clear();
        assert!(world.vx().iter().all(|&velocity| velocity == 0));
        assert!(world.vy().iter().all(|&velocity| velocity == 0));
        assert!(world.vy_frac().iter().all(|&velocity| velocity == 0));
        assert!(world.y_frac().iter().all(|&offset| offset == 0));
    }

    #[test]
    fn structural_translation_carries_velocity() {
        let mut world = World::new(3, 3);
        world.paint(1, 0, Wood);
        world.set_velocity(1, 0, 2, 3);
        let source = world.idx(1, 0);
        world.vy_frac[source] = 1;
        world.y_frac[source] = 2;
        world.paint(1, 1, Water);
        world.set_velocity(1, 1, -2, 1);
        let target = world.idx(1, 1);
        world.vy_frac[target] = -1;
        world.y_frac[target] = -2;
        let mut membership = vec![false; world.width * world.height];
        membership[source] = true;

        world.translate_structural_component(Wood, &[source], &membership, 0, 1);

        assert_eq!(world.get(1, 0), Water);
        assert_eq!(world.velocity_at(1, 0), (-2, 1));
        assert_eq!((world.vy_frac[source], world.y_frac[source]), (-1, -2));
        assert_eq!(world.get(1, 1), Wood);
        assert_eq!(world.velocity_at(1, 1), (2, 3));
        assert_eq!((world.vy_frac[target], world.y_frac[target]), (1, 2));
    }

    // --- Phase B velocity-driven movement tests ---

    #[test]
    fn velocity_driven_horizontal_move() {
        let mut world = World::new(5, 3);
        world.paint(1, 1, Sand);
        world.set_velocity(1, 1, 2, 0);
        // Manually invoke the velocity path (as step_powder would).
        let moved = world.try_velocity_move(1, 1);
        assert!(moved, "sand with vx=2 should move");
        assert_eq!(world.get(1, 1), Empty, "source cleared");
        assert_eq!(world.get(3, 1), Sand, "target cell receives sand");
        assert_eq!(world.velocity_at(3, 1), (2, 0), "velocity carried");
    }

    #[test]
    fn velocity_driven_diagonal_traversal() {
        let mut world = World::new(6, 6);
        // Sand at (1,1) with velocity (3,2) visits (2,1), (3,2), and
        // (4,3) before landing at the velocity target.
        world.paint(1, 1, Sand);
        world.set_velocity(1, 1, 3, 2);
        let moved = world.try_velocity_move(1, 1);
        assert!(moved, "sand with vx=3,vy=2 should move diagonally");
        assert_eq!(world.get(1, 1), Empty, "source cleared");
        assert_eq!(world.get(4, 3), Sand, "ends at Bresenham target (4,3)");
        // All intermediate cells are traversed via swap, so each should carry
        // the same velocity.
        assert_eq!(world.velocity_at(4, 3), (3, 2), "velocity preserved");
    }

    #[test]
    fn no_tunneling_through_thin_barrier() {
        let mut world = World::new(5, 3);
        // Sand at x=0, barrier at x=2 (Stone), target empty at x=4.
        world.paint(0, 1, Sand);
        world.set_velocity(0, 1, 4, 0);
        world.paint(2, 1, Stone);
        let moved = world.try_velocity_move(0, 1);
        // The DDA visits cells (1,1) → (2,1) and stops at the barrier.
        // Sand should end up at x=1 (the cell before the barrier).
        assert!(moved, "should move before the barrier");
        assert_eq!(world.get(0, 1), Empty, "source cleared");
        assert_eq!(world.get(1, 1), Sand, "sand stopped just before barrier");
        assert_eq!(world.get(2, 1), Stone, "barrier intact");
        assert_eq!(world.get(3, 1), Empty, "beyond barrier untouched");
        // x-component should be zeroed on collision.
        assert_eq!(world.velocity_at(1, 1), (0, 0), "vx zeroed on collision");
    }

    #[test]
    fn no_tunneling_out_of_bounds() {
        let mut world = World::new(3, 3);
        world.paint(2, 1, Sand);
        world.set_velocity(2, 1, 2, 0);
        let moved = world.try_velocity_move(2, 1);
        // vx=2 would exit the grid at x=4, so only the first step (x=3, OOB)
        // is attempted and fails → sand stays, vx zeroed.
        assert!(!moved, "should not move out of bounds");
        assert_eq!(world.get(2, 1), Sand, "sand stays in place");
        assert_eq!(world.velocity_at(2, 1), (0, 0), "vx zeroed on OOB");
    }

    #[test]
    fn velocity_blocked_components_are_zeroed() {
        let mut world = World::new(3, 3);
        world.paint(0, 0, Sand);
        world.set_velocity(0, 0, 1, 1);
        world.paint(1, 0, Stone);
        world.paint(0, 1, Stone);

        let moved = world.try_velocity_move(0, 0);

        assert!(!moved, "blocked diagonal should not move");
        assert_eq!(world.get(0, 0), Sand);
        assert_eq!(world.velocity_at(0, 0), (0, 0));
    }

    #[test]
    fn diagonal_velocity_does_not_tunnel_through_a_corner() {
        let mut world = World::new(3, 3);
        world.paint(0, 0, Sand);
        world.set_velocity(0, 0, 1, 1);
        world.paint(1, 0, Stone);

        assert!(world.try_velocity_move(0, 0));
        assert_eq!(world.get(0, 1), Sand, "vertical component may continue");
        assert_eq!(world.get(1, 1), Empty, "occupied corner was not skipped");
        assert_eq!(world.velocity_at(0, 1), (0, 1));
    }

    #[test]
    fn velocity_moved_tick_set_on_move() {
        let mut world = World::new(4, 3);
        world.tick = 42;
        // paint does NOT set moved_tick (it remains u64::MAX from init),
        // but set_velocity also doesn't touch moved_tick.
        world.paint(0, 1, Water);
        world.set_velocity(0, 1, 2, 0);
        let si = world.idx(0, 1);
        assert_eq!(
            world.moved_tick[si],
            u64::MAX,
            "paint does not set moved_tick"
        );

        let moved = world.try_velocity_move(0, 1);
        assert!(moved);
        let ti = world.idx(2, 1);
        assert_eq!(
            world.moved_tick[ti], 42,
            "destination marked moved this tick (via swap)"
        );
        assert_eq!(
            world.moved_tick[si], 42,
            "source also marked (swap semantics)"
        );
    }

    #[test]
    fn zero_velocity_does_not_trigger_move() {
        let mut world = World::new(3, 3);
        world.paint(1, 1, Sand);
        assert_eq!(world.velocity_at(1, 1), (0, 0));
        let moved = world.try_velocity_move(1, 1);
        assert!(!moved, "zero velocity should not move");
        assert_eq!(world.get(1, 1), Sand, "sand stays");
    }

    #[test]
    fn structural_material_not_moved_by_velocity() {
        let mut world = World::new(3, 3);
        world.paint(1, 1, Stone);
        world.set_velocity(1, 1, 2, 0);
        let moved = world.try_velocity_move(1, 1);
        assert!(
            !moved,
            "Stone is not fluid, should not be moved by velocity"
        );
        assert_eq!(world.get(1, 1), Stone, "stone stays");
    }

    #[test]
    fn gas_velocity_moves_horizontally_into_empty() {
        let mut world = World::new(4, 3);
        world.paint(1, 1, Smoke);
        world.set_velocity(1, 1, 2, 0);
        let moved = world.try_velocity_move(1, 1);
        assert!(moved, "smoke with vx=2 should move");
        assert_eq!(world.get(1, 1), Empty);
        assert_eq!(world.get(3, 1), Smoke);
    }

    #[test]
    fn velocity_downward_displaces_lighter_fluid() {
        let mut world = World::new(4, 5);
        // Sand (density 5) above Water (density 3). Sand with vy=2 should
        // sink through water via displacement.
        world.paint(1, 1, Sand);
        world.set_velocity(1, 1, 0, 2);
        world.paint(1, 2, Water);
        world.paint(1, 3, Water);
        let moved = world.try_velocity_move(1, 1);
        assert!(moved, "sand should sink through water via velocity");
        // After swap-chain: sand was at (1,1), swapped with (1,2) → water
        // now at (1,1). Then sand at (1,2) swapped with (1,3) → water at
        // (1,2). Final: sand at (1,3), water at (1,1) and (1,2).
        assert_eq!(world.get(1, 3), Sand, "sand at bottom of water column");
        assert!(world.get(1, 1).is_liquid(), "water displaced to top");
        assert!(world.get(1, 2).is_liquid(), "water displaced to middle");
    }

    #[test]
    fn diagonal_velocity_displaces_lighter_fluid_after_horizontal_step() {
        let mut world = World::new(3, 3);
        world.paint(0, 0, Sand);
        world.set_velocity(0, 0, 1, 1);
        world.paint(1, 1, Water);

        assert!(world.try_velocity_move(0, 0));
        assert_eq!(world.get(1, 1), Sand);
        assert_eq!(world.get(1, 0), Water);
    }

    #[test]
    fn world_step_uses_velocity_and_processes_the_cell_once() {
        let mut world = World::new(6, 6);
        world.paint(1, 1, Sand);
        world.set_velocity(1, 1, 3, 0);

        world.step();

        assert_eq!(world.get(4, 1), Sand);
        assert_eq!(world.velocity_at(4, 1), (3, 0));
        let i = world.idx(4, 1);
        assert_eq!((world.vy_frac[i], world.y_frac[i]), (1, 1));
        assert_eq!(world.grid.iter().filter(|&&m| m == Sand).count(), 1);
    }

    #[test]
    fn powder_gravity_accumulates_before_moving() {
        let mut world = World::new(7, 6);
        world.paint(3, 1, Sand);

        world.step();
        let i = world.idx(3, 1);
        assert_eq!(world.get(3, 1), Sand);
        assert_eq!((world.vy[i], world.vy_frac[i], world.y_frac[i]), (0, 1, 1));

        world.step();
        let i = world.idx(3, 1);
        assert_eq!(world.get(3, 1), Sand);
        assert_eq!((world.vy[i], world.vy_frac[i], world.y_frac[i]), (0, 2, 3));

        world.step();
        let i = world.idx(3, 2);
        assert_eq!(world.get(3, 2), Sand);
        assert_eq!((world.vy[i], world.vy_frac[i], world.y_frac[i]), (0, 3, 2));
    }

    #[test]
    fn acceleration_reaches_and_holds_terminal_velocity() {
        let mut world = World::new(5, 100);
        world.paint(2, 1, Sand);

        for _ in 0..16 {
            world.step();
        }
        let i = world
            .grid
            .iter()
            .position(|&material| material == Sand)
            .expect("sand remains in world");
        assert_eq!((world.vy[i], world.vy_frac[i]), (MAX_VELOCITY, 0));

        world.step();
        let i = world
            .grid
            .iter()
            .position(|&material| material == Sand)
            .expect("sand remains in world");
        assert_eq!((world.vy[i], world.vy_frac[i]), (MAX_VELOCITY, 0));
    }

    #[test]
    fn upward_impulse_pauses_at_apex_then_reverses() {
        let mut world = World::new(5, 20);
        world.paint(2, 10, Sand);
        world.set_velocity(2, 10, 0, -1);

        for _ in 0..2 {
            world.step();
        }
        assert_eq!(world.get(2, 9), Sand, "upward impulse should lift sand");

        for _ in 0..2 {
            world.step();
        }
        let i = world.idx(2, 9);
        assert_eq!(world.get(2, 9), Sand, "sand should pause at its apex");
        assert_eq!((world.vy[i], world.vy_frac[i]), (0, 0));

        for _ in 0..3 {
            world.step();
        }
        assert_eq!(world.get(2, 10), Sand, "gravity should reverse the impulse");
    }

    #[test]
    fn gas_buoyancy_accumulates_before_moving() {
        let mut world = World::new(5, 8);
        world.paint(2, 6, Steam);

        world.step();
        assert_eq!(world.get(2, 6), Steam);
        world.step();
        assert_eq!(world.get(2, 6), Steam);
        world.step();
        assert_eq!(world.get(2, 5), Steam);
        let i = world.idx(2, 5);
        assert_eq!(
            (world.vy[i], world.vy_frac[i], world.y_frac[i]),
            (0, -3, -2)
        );
    }

    #[test]
    fn collision_resets_generated_vertical_velocity() {
        let mut world = World::new(3, 5);
        world.paint(1, 1, Sand);
        for x in 0..3 {
            world.paint(x, 2, Metal);
        }

        for _ in 0..3 {
            world.step();
        }

        let i = world
            .grid
            .iter()
            .position(|&material| material == Sand)
            .expect("sand remains in world");
        let (sx, sy) = (i % world.width, i / world.width);
        assert_eq!((sx, sy), (1, 1));
        assert_eq!(world.velocity_at(sx, sy), (0, 0));
        assert_eq!((world.vy_frac[i], world.y_frac[i]), (0, 0));
        assert_eq!(world.get(1, 2), Metal);
    }

    #[test]
    fn fling_outward_sets_outward_velocity_and_clears_source() {
        let mut world = World::new(9, 3);
        world.paint(2, 1, Sand);
        world.set_velocity(2, 1, 0, 0);

        assert!(world.fling_outward(2, 1, 1, 0));
        assert_eq!(world.get(2, 1), Empty, "source cleared");
        assert_eq!(world.velocity_at(2, 1), (0, 0), "source velocity cleared");
        // Fling along (1, 0) -> candidate (2, 0) chosen, so impulse = (2, 0).
        let (new_vx, new_vy) = world.velocity_at(4, 1);
        assert_eq!(
            (new_vx, new_vy),
            (2, 0),
            "fling sets outward velocity from impulse (2,0)"
        );
        assert_eq!(world.get(4, 1), Sand, "sand at displaced location");

        // Existing velocity should be combined with impulse.
        let mut world2 = World::new(9, 3);
        world2.paint(2, 1, Water);
        world2.set_velocity(2, 1, 1, -1);
        assert!(world2.fling_outward(2, 1, 1, 0));
        let (vx2, vy2) = world2.velocity_at(4, 1);
        // Existing (1, -1) + impulse (2, 0) = (3, -1) — within MAX_VELOCITY.
        assert_eq!(
            (vx2, vy2),
            (3, -1),
            "existing velocity combined with outward impulse"
        );
    }
}
