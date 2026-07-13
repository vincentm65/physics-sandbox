use rand::Rng;

use crate::material::{AMBIENT_TEMP, Material};
use crate::scene_manager::SceneState;
use Material::*;

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

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Lifetime bounds (in ticks).
const FIRE_LIFE_MIN: u16 = 60;
const FIRE_LIFE_MAX: u16 = 100;
const EMBER_LIFE_MIN: u16 = 80;
const EMBER_LIFE_MAX: u16 = 170;
const STEAM_LIFE_MIN: u16 = 120;
const STEAM_LIFE_MAX: u16 = 280;
const SMOKE_LIFE_MIN: u16 = 80;
const SMOKE_LIFE_MAX: u16 = 180;
const GUNPOWDER_BLAST_RADIUS: i32 = 5;
const TNT_BLAST_RADIUS: i32 = 10;
const C4_BLAST_RADIUS: i32 = 12;
/// Ticks a fuse cell smoulders before flaring to fire and kindling its
/// neighbours. Sets the burn-front pace: one cell advances per this many ticks.
pub(crate) const FUSE_BURN_TICKS: u16 = 3;

const CHUNK_W: usize = 64;
const CHUNK_H: usize = 32;

/// Fraction of cooled embers that leave a residue of ash; the rest are fully
/// consumed by the burn.
const ASH_CHANCE_PER_MILLE: u32 = 50;

/// How far a liquid tries to flow sideways in one tick. Bigger = flatter water;
/// lava stays viscous.
fn spread_of(m: Material) -> usize {
    match m {
        Water | LiquidNitrogen => 6,
        Acid | Oil => 4,
        _ => 1,
    }
}

/// Maximum number of downward cells a material can traverse in one tick.
/// Faster materials still check every intermediate cell, so thin floors cannot
/// be skipped.
fn fall_speed_of(m: Material) -> usize {
    match m {
        Water | LiquidNitrogen | Mercury | Sand | Salt | Gunpowder => 2,
        _ => 1,
    }
}

fn chunks_x(width: usize) -> usize {
    width.div_ceil(CHUNK_W)
}

fn chunks_y(height: usize) -> usize {
    height.div_ceil(CHUNK_H)
}

fn chunks_len(width: usize, height: usize) -> usize {
    chunks_x(width) * chunks_y(height)
}

fn activate_chunk_neighborhood(
    width: usize,
    height: usize,
    chunks_x: usize,
    x: usize,
    y: usize,
    chunks: &mut [bool],
) {
    if width == 0 || height == 0 || chunks_x == 0 || chunks.is_empty() {
        return;
    }

    let x0 = x.saturating_sub(1) / CHUNK_W;
    let y0 = y.saturating_sub(1) / CHUNK_H;
    let x1 = (x + 1).min(width - 1) / CHUNK_W;
    let y1 = (y + 1).min(height - 1) / CHUNK_H;

    for cy in y0..=y1 {
        for cx in x0..=x1 {
            if let Some(chunk) = chunks.get_mut(cy * chunks_x + cx) {
                *chunk = true;
            }
        }
    }
}

/// A falling-sand style cellular world.
pub struct World {
    pub width: usize,
    pub height: usize,
    grid: Vec<Material>,
    life: Vec<u16>,
    seed: Vec<u8>,
    /// Approximate Celsius temperature per cell.
    temp: Vec<i16>,
    /// Scratch buffer for heat diffusion.
    temp_next: Vec<i16>,
    moved_tick: Vec<u64>,
    active_chunks: Vec<bool>,
    next_active_chunks: Vec<bool>,
    /// Scratch space for connected-component wood physics.
    wood_seen: Vec<bool>,
    chunks_x: usize,
    chunks_y: usize,
    tick: u64,
    scene: Scene,
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
            temp: vec![AMBIENT_TEMP; n],
            temp_next: vec![AMBIENT_TEMP; n],
            moved_tick: vec![u64::MAX; n],
            active_chunks: vec![false; chunks_len(width, height)],
            next_active_chunks: vec![false; chunks_len(width, height)],
            wood_seen: vec![false; n],
            chunks_x: chunks_x(width),
            chunks_y: chunks_y(height),
            tick: 0,
            scene: Scene::House,
        }
    }

    fn idx(&self, x: usize, y: usize) -> usize {
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
    pub fn temp_at(&self, x: usize, y: usize) -> i16 {
        self.temp[self.idx(x, y)]
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
        self.tick = 0;
        self.moved_tick.fill(u64::MAX);
        self.active_chunks.fill(false);
        self.next_active_chunks.fill(false);
        self.wood_seen.fill(false);
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
        let mut temp = vec![AMBIENT_TEMP; width * height];
        let cw = width.min(self.width);
        let ch = height.min(self.height);
        for y in 0..ch {
            for x in 0..cw {
                grid[y * width + x] = self.grid[y * self.width + x];
                life[y * width + x] = self.life[y * self.width + x];
                seed[y * width + x] = self.seed[y * self.width + x];
                temp[y * width + x] = self.temp[y * self.width + x];
            }
        }
        self.width = width;
        self.height = height;
        self.grid = grid;
        self.life = life;
        self.seed = seed;
        self.temp = temp;
        self.temp_next = vec![AMBIENT_TEMP; width * height];
        self.moved_tick = vec![u64::MAX; width * height];
        self.chunks_x = chunks_x(width);
        self.chunks_y = chunks_y(height);
        self.active_chunks = vec![true; chunks_len(width, height)];
        self.next_active_chunks = vec![false; chunks_len(width, height)];
        self.wood_seen = vec![false; width * height];
    }

    // --- internal mutation helpers (used only during a step) ---

    fn activate_idx(&mut self, i: usize) {
        let x = i % self.width;
        let y = i / self.width;
        self.activate_next(x, y);
    }

    fn activate_now(&mut self, x: usize, y: usize) {
        activate_chunk_neighborhood(
            self.width,
            self.height,
            self.chunks_x,
            x,
            y,
            &mut self.active_chunks,
        );
    }

    fn activate_next(&mut self, x: usize, y: usize) {
        activate_chunk_neighborhood(
            self.width,
            self.height,
            self.chunks_x,
            x,
            y,
            &mut self.next_active_chunks,
        );
    }

    fn put(&mut self, i: usize, m: Material, life: u16) {
        let prev_temp = self.temp[i];
        self.grid[i] = m;
        self.life[i] = life;
        self.seed[i] = rand::random();
        // Heat sources clamp; everything else keeps the cell's thermal history so
        // phase changes (ice→water, lava→stone) stay continuous instead of
        // snapping back to ambient.
        self.temp[i] = m.heat_source_temp().unwrap_or(prev_temp);
        self.moved_tick[i] = self.tick;
        self.activate_idx(i);
    }

    fn swap(&mut self, a: usize, b: usize) {
        self.grid.swap(a, b);
        self.life.swap(a, b);
        self.seed.swap(a, b);
        self.temp.swap(a, b);
        self.moved_tick[a] = self.tick;
        self.moved_tick[b] = self.tick;
        self.activate_idx(a);
        self.activate_idx(b);
    }

    fn adj(&self, x: usize, y: usize, dx: i32, dy: i32) -> Option<(usize, usize)> {
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
    fn try_step(
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

    fn try_into(
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

    /// Move downward through at most `steps` adjacent cells. Each intermediate
    /// cell is checked and swapped, preserving collision and displacement rules.
    fn try_fall(
        &mut self,
        x: usize,
        y: usize,
        steps: usize,
        allow: impl Fn(Material) -> bool,
    ) -> bool {
        let mut cy = y;
        let mut moved = false;
        for _ in 0..steps {
            let Some((_, ty)) = self.adj(x, cy, 0, 1) else {
                break;
            };
            if !self.try_into(x, cy, x, ty, &allow) {
                break;
            }
            cy = ty;
            moved = true;
        }
        moved
    }

    /// Score horizontal room in one direction, preferring routes that lead to
    /// a downward opening. This approximates local liquid pressure without a
    /// separate pressure grid.
    fn flow_score(&self, x: usize, y: usize, dir: i32, range: usize) -> usize {
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

    fn noise(&self, x: usize, y: usize, salt: u32) -> u32 {
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

    fn dirs(&self, x: usize, y: usize, salt: u32) -> (i32, i32) {
        if self.noise(x, y, salt) & 1 == 0 {
            (-1, 1)
        } else {
            (1, -1)
        }
    }

    fn chance(&self, x: usize, y: usize, salt: u32, per_mille: u32) -> bool {
        self.noise(x, y, salt) % 1000 < per_mille
    }

    fn chance_idx(&self, i: usize, salt: u32, per_mille: u32) -> bool {
        self.chance(i % self.width, i / self.width, salt, per_mille)
    }

    fn roll(&self, x: usize, y: usize, salt: u32) -> u32 {
        self.noise(x, y, salt) % 1000
    }

    fn n4(&self, x: usize, y: usize) -> [Option<(usize, usize)>; 4] {
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

    fn n8(&self, x: usize, y: usize) -> [Option<(usize, usize)>; 8] {
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

    fn wood_can_displace(&self, m: Material) -> bool {
        m.is_empty() || m.is_gas() || (m.is_fluid() && Wood.density() > m.density())
    }

    fn component_can_translate(
        &self,
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
            in_component[ti] || self.wood_can_displace(self.grid[ti])
        })
    }

    fn translate_wood_component(
        &mut self,
        component: &[usize],
        in_component: &[bool],
        dx: i32,
        dy: i32,
    ) {
        let mut ordered = component.to_vec();
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

        let mut target_set = vec![false; self.grid.len()];
        let mut moves = Vec::with_capacity(ordered.len());
        let mut displaced = Vec::new();
        for &i in &ordered {
            let x = i % self.width;
            let y = i / self.width;
            let (tx, ty) = self.adj(x, y, dx, dy).expect("prechecked wood translation");
            let ti = self.idx(tx, ty);
            target_set[ti] = true;
            moves.push((i, ti, self.life[i], self.seed[i], self.temp[i]));
            if !in_component[ti] && self.grid[ti] != Empty {
                displaced.push((self.grid[ti], self.life[ti], self.seed[ti], self.temp[ti]));
            }
        }

        for &(i, _, _, _, _) in &moves {
            self.grid[i] = Empty;
            self.life[i] = 0;
            self.temp[i] = AMBIENT_TEMP;
            self.moved_tick[i] = self.tick;
            self.activate_idx(i);
        }
        for &(_, ti, life, seed, temp) in &moves {
            self.grid[ti] = Wood;
            self.life[ti] = life;
            self.seed[ti] = seed;
            self.temp[ti] = temp;
            self.moved_tick[ti] = self.tick;
            self.activate_idx(ti);
        }
        for &(i, _, _, _, _) in &moves {
            if target_set[i] {
                continue;
            }
            if let Some((m, life, seed, temp)) = displaced.pop() {
                self.grid[i] = m;
                self.life[i] = life;
                self.seed[i] = seed;
                self.temp[i] = temp;
            }
        }
    }

    fn try_translate_wood_component(
        &mut self,
        component: &[usize],
        in_component: &[bool],
        dx: i32,
        dy: i32,
    ) -> bool {
        if !self.component_can_translate(component, in_component, dx, dy) {
            return false;
        }
        self.translate_wood_component(component, in_component, dx, dy);
        true
    }

    /// Move each connected wood island down one cell when every destination is
    /// empty or a lighter fluid. Supported islands remain stationary.
    fn step_wood_components(&mut self) {
        let n = self.grid.len();
        self.wood_seen.fill(false);
        let mut stack = Vec::new();
        let mut component = Vec::new();
        let mut in_component = vec![false; n];

        for y in (0..self.height).rev() {
            for x in 0..self.width {
                let start = self.idx(x, y);
                if self.grid[start] != Wood
                    || self.moved_tick[start] == self.tick
                    || self.wood_seen[start]
                {
                    continue;
                }

                stack.clear();
                component.clear();
                stack.push(start);
                self.wood_seen[start] = true;

                while let Some(i) = stack.pop() {
                    component.push(i);
                    let cx = i % self.width;
                    let cy = i / self.width;
                    for n in self.n4(cx, cy) {
                        let Some((nx, ny)) = n else { continue };
                        let ni = self.idx(nx, ny);
                        if self.grid[ni] == Wood
                            && self.moved_tick[ni] != self.tick
                            && !self.wood_seen[ni]
                        {
                            self.wood_seen[ni] = true;
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
                        in_component[below] || self.wood_can_displace(self.grid[below])
                    }
                });

                if unsupported {
                    let _ = self.try_translate_wood_component(&component, &in_component, 0, 1);
                }

                for &i in &component {
                    in_component[i] = false;
                }
            }
        }
    }

    // --- the step ---

    pub fn step(&mut self) {
        self.next_active_chunks.fill(false);
        self.step_heat();
        self.step_wood_components();

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
                        if material.flammable() {
                            self.activate_next(x, y);
                            if self.step_combustible(x, y) {
                                continue;
                            }
                        } else if material != Ice
                            && material
                                .melt()
                                .is_some_and(|(melt_temp, _, _)| {
                                    self.effective_temp(x, y).max(0) as u16 >= melt_temp / 2
                                })
                        {
                            // Only track heat-soak when the cell is already warm enough
                            // that melting is plausible (avoids keeping cold sand active).
                            self.activate_next(x, y);
                            if self.step_melt(x, y) {
                                continue;
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
                            Sand | Ash | Salt | Gunpowder | Coal => self.step_powder(x, y),
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

    /// Diffuse heat through active chunks. Sources clamp; other cells equalize
    /// with neighbours and slowly return toward ambient.
    fn step_heat(&mut self) {
        self.temp_next.copy_from_slice(&self.temp);
        for chunk_y in 0..self.chunks_y {
            for chunk_x in 0..self.chunks_x {
                let chunk_i = chunk_y * self.chunks_x + chunk_x;
                if !self.active_chunks.get(chunk_i).copied().unwrap_or(false) {
                    continue;
                }
                let y0 = chunk_y * CHUNK_H;
                let y1 = ((chunk_y + 1) * CHUNK_H).min(self.height);
                let x0 = chunk_x * CHUNK_W;
                let x1 = ((chunk_x + 1) * CHUNK_W).min(self.width);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = self.idx(x, y);
                        let m = self.grid[i];
                        if let Some(src) = m.heat_source_temp() {
                            self.temp_next[i] = src;
                            self.activate_next(x, y);
                            continue;
                        }
                        let mut sum = self.temp[i] as i32;
                        let mut n = 1i32;
                        for neigh in self.n4(x, y).into_iter().flatten() {
                            sum += self.temp[self.idx(neigh.0, neigh.1)] as i32;
                            n += 1;
                        }
                        let avg = sum / n;
                        let cond = m.thermal_conductivity() as i32;
                        let cur = self.temp[i] as i32;
                        let mut next = cur + (avg - cur) * cond / 16;
                        next += (AMBIENT_TEMP as i32 - next) / 48;
                        self.temp_next[i] = next.clamp(-200, 1_500) as i16;
                        if (self.temp_next[i] - AMBIENT_TEMP).abs() > 5 {
                            self.activate_next(x, y);
                        }
                    }
                }
            }
        }
        std::mem::swap(&mut self.temp, &mut self.temp_next);
    }

    /// Peak temperature felt by a cell: stored heat plus direct contact with
    /// hot/cold sources (so ignition still works before heat soaks through).
    fn effective_temp(&self, x: usize, y: usize) -> i16 {
        let i = self.idx(x, y);
        let mut t = self.temp[i];
        for n in self.n8(x, y).into_iter().flatten() {
            let ni = self.idx(n.0, n.1);
            let m = self.grid[ni];
            if let Some(src) = m.heat_source_temp() {
                if src > t {
                    t = src;
                } else if src < t {
                    t = t.min(src + (t - src) / 2);
                }
            } else if !m.is_empty() && !m.is_gas() {
                // Conductive contact with solids/liquids/powders: feel their
                // stored heat immediately (hot metal melts ice, boils water).
                // Only hotter bodies raise felt temp — ambient-temperature stone
                // must not suppress boiling of already-hot water.
                let nt = self.temp[ni];
                if nt > t {
                    t = nt;
                } else if nt < 0 && nt < t {
                    // Sub-zero bodies (chilled metal, etc.) still cool on contact.
                    t = t.min(nt + (t - nt) / 2);
                }
            }
        }
        t
    }

    fn explode(&mut self, x: usize, y: usize, radius: i32) {
        let r2 = radius * radius;
        let mut cells: Vec<(i32, i32, i32, usize, usize)> = Vec::new();
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let dist2 = dx * dx + dy * dy;
                if dist2 > r2 {
                    continue;
                }
                let Some((tx, ty)) = self.adj(x, y, dx, dy) else {
                    continue;
                };
                cells.push((dist2, dx, dy, tx, ty));
            }
        }
        // Outside-in so flung grains find space cleared by outer destruction.
        cells.sort_unstable_by(|a, b| b.0.cmp(&a.0));

        for &(dist2, dx, dy, tx, ty) in &cells {
            let i = self.idx(tx, ty);
            let material = self.grid[i];
            if material.blast_resistant() {
                continue;
            }
            if let Some(shard) = material.blast_shatter_product() {
                self.put(i, shard, 0);
                continue;
            }

            // Fling loose powders/fluids outward before deciding destruction.
            if material.is_fluid() && dist2 > 0 && self.fling_outward(tx, ty, dx, dy) {
                continue;
            }

            let roll = self.roll(tx, ty, 0x92);
            if roll < 550 {
                self.put(i, Fire, rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX));
            } else if roll < 800 {
                self.put(i, Smoke, rand_range(SMOKE_LIFE_MIN / 2, SMOKE_LIFE_MAX / 2));
            } else if roll < 920
                && matches!(
                    material,
                    Stone | Wood | Concrete | Plant | Ice | Glass | Coal | Ash
                )
            {
                // Outer rubble instead of pure vaporization.
                self.put(i, Sand, 0);
            } else {
                self.put(i, Empty, 0);
            }
        }
    }

    /// Push the cell at `(x, y)` further from the blast origin along `(dx, dy)`.
    /// Returns true when the grain left its original cell.
    fn fling_outward(&mut self, x: usize, y: usize, dx: i32, dy: i32) -> bool {
        let sx = dx.signum();
        let sy = dy.signum();
        let i = self.idx(x, y);
        let material = self.grid[i];
        let life = self.life[i];
        let seed = self.seed[i];
        let temp = self.temp[i];

        // Prefer longer throws, then cardinal fallbacks when the diagonal is blocked.
        let candidates = [
            (sx * 2, sy * 2),
            (sx, sy),
            (sx * 2, 0),
            (0, sy * 2),
            (sx, 0),
            (0, sy),
        ];
        for idx in 0..candidates.len() {
            let (px, py) = candidates[idx];
            if px == 0 && py == 0 {
                continue;
            }
            if candidates[..idx].contains(&(px, py)) {
                continue;
            }
            let Some((lx, ly)) = self.adj(x, y, px, py) else {
                continue;
            };
            let li = self.idx(lx, ly);
            if !(self.grid[li].is_empty() || self.grid[li].is_gas()) {
                continue;
            }
            self.grid[i] = Empty;
            self.life[i] = 0;
            self.temp[i] = AMBIENT_TEMP;
            self.moved_tick[i] = self.tick;
            self.activate_idx(i);

            self.grid[li] = material;
            self.life[li] = life;
            self.seed[li] = seed;
            self.temp[li] = temp;
            self.moved_tick[li] = self.tick;
            self.activate_idx(li);
            return true;
        }
        false
    }

    fn step_combustible(&mut self, x: usize, y: usize) -> bool {
        let i = self.idx(x, y);
        let material = self.grid[i];
        let Some((ignition_temperature, ignition_delay, burn_life)) = material.combustion() else {
            return false;
        };
        let heat = self.effective_temp(x, y).max(0) as u16;

        if heat < ignition_temperature {
            self.life[i] = 0;
            return false;
        }

        self.life[i] = self.life[i].saturating_add(1);
        if self.life[i] < ignition_delay {
            return false;
        }

        self.put(i, material.burn_product(), burn_life);
        true
    }

    /// Soak heat into structural materials until they crack/melt into a product.
    fn step_melt(&mut self, x: usize, y: usize) -> bool {
        let i = self.idx(x, y);
        let material = self.grid[i];
        // Ice melting is handled in step_ice (contact + temperature).
        if material == Ice {
            return false;
        }
        let Some((melt_temp, delay, product)) = material.melt() else {
            return false;
        };
        let heat = self.effective_temp(x, y).max(0) as u16;
        if heat < melt_temp {
            self.life[i] = 0;
            return false;
        }
        self.life[i] = self.life[i].saturating_add(1);
        if self.life[i] < delay {
            return false;
        }
        let life = rand_life(product);
        self.put(i, product, life);
        true
    }

    fn step_tnt(&mut self, x: usize, y: usize) {
        if self.is_heated(x, y) {
            self.explode(x, y, TNT_BLAST_RADIUS);
        }
    }

    fn step_fuse(&mut self, x: usize, y: usize) {
        self.activate_next(x, y);
        let i = self.idx(x, y);

        // Dormant fuse: light it only from an external heat source. Propagation
        // along the fuse is push-driven by burning cells (below), so a single
        // spark kindles one cell and the burn front then walks the rest at a
        // fixed pace instead of flashing the whole component in a single tick.
        if self.life[i] == 0 {
            if self.is_heated(x, y) {
                self.life[i] = FUSE_BURN_TICKS;
                self.moved_tick[i] = self.tick;
                self.activate_idx(i);
            }
            return;
        }

        // Burning fuse: smoulder down, then flare to fire and kindle the next
        // layer of dormant fuse neighbours so the front advances one cell per
        // FUSE_BURN_TICKS ticks. Marking freshly lit cells as moved this tick
        // keeps the front to one layer per tick regardless of scan order.
        self.life[i] = self.life[i].saturating_sub(1);
        if self.life[i] != 0 {
            return;
        }
        self.put(i, Fire, rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX));
        for n in self.n8(x, y) {
            let Some((nx, ny)) = n else {
                continue;
            };
            let ni = self.idx(nx, ny);
            if self.grid[ni] == Fuse && self.life[ni] == 0 {
                self.life[ni] = FUSE_BURN_TICKS;
                self.moved_tick[ni] = self.tick;
                self.activate_idx(ni);
            }
        }
    }

    fn is_heated(&self, x: usize, y: usize) -> bool {
        self.effective_temp(x, y) >= 600
            || self
                .n8(x, y)
                .into_iter()
                .flatten()
                .any(|(nx, ny)| matches!(self.grid[self.idx(nx, ny)], Fire | Lava | Ember))
    }

    fn step_c4(&mut self, x: usize, y: usize) {
        self.activate_next(x, y);
        if self.is_heated(x, y) {
            self.explode(x, y, C4_BLAST_RADIUS);
        }
    }

    fn step_powder(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);

        if self.react_cell(x, y) {
            return;
        }
        let m = self.grid[i];

        // Gunpowder blast respects blast_resistant materials via explode().
        if m == Gunpowder && self.is_heated(x, y) {
            self.explode(x, y, GUNPOWDER_BLAST_RADIUS);
            return;
        }

        let sink = |other| m.can_sink_into(other);
        // Downward, with bounded per-material speed.
        if self.try_fall(x, y, fall_speed_of(m), sink) {
            return;
        }
        // Diagonals
        let (d1, d2) = self.dirs(x, y, 0x10);
        for d in [d1, d2] {
            if let Some((tx, ty)) = self.adj(x, y, d, 1) {
                let ti = self.idx(tx, ty);
                if sink(self.grid[ti]) {
                    self.swap(i, ti);
                    return;
                }
            }
        }
    }

    fn step_liquid(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);

        // reactions first (may consume this cell)
        if self.react_cell(x, y) {
            return;
        }
        let m = self.grid[i];
        if !m.is_liquid() {
            return;
        }

        // Lava and napalm are viscous; napalm also clings to solids.
        if matches!(m, Lava | Napalm) && !self.tick.is_multiple_of(2) {
            return;
        }
        if m.sticky() {
            if let Some((bx, by)) = self.adj(x, y, 0, 1) {
                let below = self.grid[self.idx(bx, by)];
                if !below.is_empty() && !below.is_fluid() && !self.chance(x, y, 0x55, 80) {
                    return;
                }
            }
        }

        let sink = |other| m.can_sink_into(other);
        let rise = |t: Material| t.is_fluid() && m.density() < t.density();

        // Downward, with bounded per-material speed.
        if self.try_fall(x, y, fall_speed_of(m), sink) {
            return;
        }
        // Diagonals
        let (d1, d2) = self.dirs(x, y, 0x20);
        for d in [d1, d2] {
            if let Some((tx, ty)) = self.adj(x, y, d, 1) {
                let ti = self.idx(tx, ty);
                if sink(self.grid[ti]) {
                    self.swap(i, ti);
                    return;
                }
            }
        }
        // buoyancy: lighter liquid rises through a denser one
        if self.try_step(x, y, 0, -1, rise) {
            return;
        }
        // sticky gels barely flow sideways
        if m.sticky() && !self.chance(x, y, 0x56, 120) {
            return;
        }
        // horizontal flow: travel several cells so a liquid levels out quickly
        // instead of piling into a thin column.
        let spread = spread_of(m);
        let empty = |t: Material| t.is_empty();
        let (random_first, random_second) = self.dirs(x, y, 0x21);
        let left_score = self.flow_score(x, y, -1, spread);
        let right_score = self.flow_score(x, y, 1, spread);
        let (d1, d2) = match left_score.cmp(&right_score) {
            std::cmp::Ordering::Greater => (-1, 1),
            std::cmp::Ordering::Less => (1, -1),
            std::cmp::Ordering::Equal => (random_first, random_second),
        };
        if self.flow(x, y, d1, spread, empty) {
            return;
        }
        let _ = self.flow(x, y, d2, spread, empty);
    }

    /// Slide the cell at `(x, y)` sideways up to `range` times in `dir`, stopping
    /// at the first cell `allow` rejects. Returns true if it moved at least once.
    fn flow(
        &mut self,
        x: usize,
        y: usize,
        dir: i32,
        range: usize,
        allow: impl Fn(Material) -> bool,
    ) -> bool {
        let mut cx = x as i32;
        for _ in 0..range {
            if !self.try_step(cx as usize, y, dir, 0, &allow) {
                break;
            }
            cx += dir;
            if self
                .adj(cx as usize, y, 0, 1)
                .is_some_and(|(bx, by)| self.get(bx, by) == Empty)
            {
                break;
            }
        }
        cx != x as i32
    }

    fn step_gas(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);
        let m = self.grid[i];
        let life = self.life[i].saturating_sub(1);
        let on_cold_surface = |world: &World, x: usize, y: usize| {
            world.n4(x, y).into_iter().flatten().any(|(nx, ny)| {
                matches!(
                    world.grid[world.idx(nx, ny)],
                    Ice | LiquidNitrogen | Metal | Glass
                ) || (world.grid[world.idx(nx, ny)] == Water
                    && world.temp[world.idx(nx, ny)] < 40)
            })
        };
        if life == 0 {
            // Spent steam rains out when cool or when touching a cold surface.
            if m == Steam && (self.temp[i] < 100 || on_cold_surface(self, x, y)) {
                self.put(i, Water, 0);
            } else {
                self.put(i, Empty, 0);
            }
            return;
        }
        self.life[i] = life;

        // Steam condenses on cold surfaces; contact is a strong cue even while
        // the gas is still modelled as a warm source cell.
        if m == Steam {
            if on_cold_surface(self, x, y) && (life < 80 || self.chance(x, y, 0x33, 250)) {
                self.put(i, Water, 0);
                return;
            }
            if self.temp[i] < 80 && (life < 40 || self.chance(x, y, 0x34, 100)) {
                self.put(i, Water, 0);
                return;
            }
        }

        let d = m.density();
        let rise = |t: Material| t.is_empty() || (t.is_fluid() && d < t.density());
        if self.try_step(x, y, 0, -1, rise) {
            return;
        }
        let (d1, d2) = self.dirs(x, y, 0x30);
        if self.try_step(x, y, d1, -1, rise) {
            return;
        }
        if self.try_step(x, y, d2, -1, rise) {
            return;
        }
        let empty = |t: Material| t.is_empty();
        let (d1, d2) = self.dirs(x, y, 0x31);
        if self.try_step(x, y, d1, 0, empty) {
            return;
        }
        let _ = self.try_step(x, y, d2, 0, empty);

        // slow dissipation
        if self.chance(x, y, 0x32, 4) {
            self.put(i, Empty, 0);
        }
    }

    fn step_fire(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);
        let life = self.life[i].saturating_sub(1);
        if life == 0 {
            self.put(i, Empty, 0);
            return;
        }
        self.life[i] = life;

        // No oxygen: a flame fully boxed in by solids smothers into smoke.
        // Embers still smoulder without free air; only open flame is gated.
        if !self.has_combustion_air(x, y) {
            self.put(
                i,
                Smoke,
                rand_range(SMOKE_LIFE_MIN / 2, SMOKE_LIFE_MAX / 2),
            );
            return;
        }

        // Oil/napalm fires shrug off water: water boils away, flame keeps burning.
        let oily = self
            .n8(x, y)
            .into_iter()
            .flatten()
            .any(|(nx, ny)| self.grid[self.idx(nx, ny)].is_oily());

        let mut extinguished = false;
        for n in self.n8(x, y) {
            let Some((nx, ny)) = n else {
                continue;
            };
            let ni = self.idx(nx, ny);
            let other = self.grid[ni];
            if other == Water {
                self.put(ni, Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
                if !oily {
                    extinguished = true;
                }
                // greasy fires: water steams off without killing the flame
            } else if other == LiquidNitrogen {
                extinguished = true;
            }
        }

        if extinguished {
            self.put(i, Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
            return;
        }

        // An occasional wisp of smoke wafts up off the flame.
        if self.chance(x, y, 0x43, 20)
            && let Some((ux, uy)) = self.adj(x, y, 0, -1)
        {
            let ui = self.idx(ux, uy);
            if self.grid[ui] == Empty {
                self.put(ui, Smoke, rand_range(SMOKE_LIFE_MIN, SMOKE_LIFE_MAX));
            }
        }

        // Rise like a hot gas, but linger ~40% of the time so it can keep
        // spreading along a fuel surface instead of floating straight up.
        if !self.chance(x, y, 0x44, 600) {
            return;
        }
        let passable = |t: Material| t.is_empty() || t == Smoke;
        // Rising flame passes through smoke before trying to spread sideways.
        if self.try_step(x, y, 0, -1, passable) {
            return;
        }
        let (d1, d2) = self.dirs(x, y, 0x40);
        if self.try_step(x, y, d1, -1, passable) {
            return;
        }
        if self.try_step(x, y, d1, 0, passable) {
            return;
        }
        let _ = self.try_step(x, y, d2, 0, passable);
    }

    /// Free air a flame can breathe: empty space, gaseous exhaust, or fuel it
    /// is actively consuming. Fully boxed by inert solids smothers to smoke.
    fn has_combustion_air(&self, x: usize, y: usize) -> bool {
        self.n8(x, y).into_iter().flatten().any(|(nx, ny)| {
            let m = self.grid[self.idx(nx, ny)];
            matches!(m, Empty | Smoke | Steam | Fire) || m.flammable()
        })
    }

    /// Only `ASH_CHANCE` of cooled embers leave a residue of ash; the rest burn
    /// away completely.
    fn residue(&mut self, i: usize) {
        if self.chance_idx(i, 0x45, ASH_CHANCE_PER_MILLE) {
            self.put(i, Ash, 0);
        } else {
            self.put(i, Empty, 0);
        }
    }

    /// A smoldering coal: the actual burning core left behind when wood catches.
    /// It glows, ignites neighbours, licks flames upward, breathes smoke, and
    /// finally cools into ash. Water quenches it instantly.
    fn step_ember(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);
        let life = self.life[i].saturating_sub(1);
        if life == 0 {
            self.residue(i);
            return;
        }
        self.life[i] = life;

        let mut quenched = false;
        for n in self.n8(x, y) {
            let Some((nx, ny)) = n else {
                continue;
            };
            let ni = self.idx(nx, ny);
            match self.grid[ni] {
                Water => {
                    self.put(ni, Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
                    quenched = true;
                }
                Empty | Smoke if ny < y => {
                    // Flames can lick upward through smoke; smoke wisps are less common.
                    let r = self.roll(nx, ny, 0x53);
                    if r < 80 {
                        self.put(ni, Fire, rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX));
                    } else if r < 100 {
                        self.put(ni, Smoke, rand_range(SMOKE_LIFE_MIN, SMOKE_LIFE_MAX));
                    }
                }
                _ => {}
            }
        }

        if quenched {
            self.residue(i);
            return;
        }

        // Embers mostly burn out where they form instead of piling up as grit.
        if self.chance(x, y, 0x54, 100) {
            let sink = |other| Ember.can_sink_into(other);
            if self.try_step(x, y, 0, 1, sink) {
                return;
            }
            let (d1, d2) = self.dirs(x, y, 0x50);
            if self.try_step(x, y, d1, 1, sink) {
                return;
            }
            let _ = self.try_step(x, y, d2, 1, sink);
        }
    }

    /// Central material chemistry. Returns true when the cell at `(x, y)` was
    /// consumed or replaced and should stop further stepping this tick.
    fn react_cell(&mut self, x: usize, y: usize) -> bool {
        let i = self.idx(x, y);
        let m = self.grid[i];
        match m {
            Lava => self.react_lava(x, y),
            Acid => self.react_acid(x, y),
            Water => self.react_water(x, y),
            LiquidNitrogen => self.react_liquid_nitrogen(x, y),
            Salt => self.react_salt(x, y),
            Mercury => self.react_mercury(x, y),
            _ => false,
        }
    }

    fn react_liquid_nitrogen(&mut self, x: usize, y: usize) -> bool {
        let i = self.idx(x, y);
        for n in self.n4(x, y).into_iter().flatten() {
            let ni = self.idx(n.0, n.1);
            match self.grid[ni] {
                Water => self.put(ni, Ice, 0),
                Fire | Ember | Lava => {
                    self.put(ni, if self.grid[ni] == Lava { Stone } else { Smoke }, 0);
                    self.put(i, Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
                    return true;
                }
                // Hot glass meets cryogenic quench → shatter.
                Glass if self.temp[ni] >= 200 => {
                    self.put(ni, Sand, 0);
                }
                _ => {}
            }
        }
        false
    }

    fn react_lava(&mut self, x: usize, y: usize) -> bool {
        let mut solidified = false;
        for n in self.n4(x, y) {
            let Some((nx, ny)) = n else {
                continue;
            };
            let ni = self.idx(nx, ny);
            match self.grid[ni] {
                Water => {
                    self.put(ni, Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
                    solidified = true;
                }
                // Sand already softened by heat can be absorbed into the flow.
                // Stone/concrete use the slower heat-soak melt path instead.
                Sand if self.chance(nx, ny, 0x72, 40) => {
                    self.put(ni, Lava, 0);
                }
                _ => {}
            }
        }
        if solidified {
            self.put(self.idx(x, y), Stone, 0);
        }
        solidified
    }

    fn react_acid(&mut self, x: usize, y: usize) -> bool {
        let mut consumed = false;
        for n in self.n4(x, y) {
            let Some((nx, ny)) = n else {
                continue;
            };
            let ni = self.idx(nx, ny);
            let other = self.grid[ni];
            let resist = other.acid_resistance();
            if resist >= 1000 {
                continue;
            }
            // base etch chance reduced by resistance
            let etch = 200u32.saturating_sub(resist / 5);
            if etch > 0 && self.chance(nx, ny, 0x70, etch) {
                self.put(ni, Empty, 0);
                if self.chance(nx, ny, 0x71, 350) {
                    consumed = true;
                }
            }
        }
        if consumed {
            self.put(self.idx(x, y), Empty, 0);
        }
        consumed
    }

    /// Salt dissolves into water (salt removed only) and brines ice into water.
    fn react_salt(&mut self, x: usize, y: usize) -> bool {
        let i = self.idx(x, y);
        for n in self.n4(x, y).into_iter().flatten() {
            let ni = self.idx(n.0, n.1);
            match self.grid[ni] {
                Water if self.chance(n.0, n.1, 0x90, 80) => {
                    // dissolve: remove salt, leave the water cell untouched
                    if self.chance(n.0, n.1, 0x91, 400) {
                        self.put(i, Empty, 0);
                        return true;
                    }
                }
                Ice if self.chance(n.0, n.1, 0x93, 120) => {
                    // brine lowers freeze point: ice thaws
                    self.put(ni, Water, 0);
                    if self.chance(n.0, n.1, 0x94, 200) {
                        self.put(i, Empty, 0);
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Mercury slowly amalgamates (eats) adjacent metal.
    fn react_mercury(&mut self, x: usize, y: usize) -> bool {
        for n in self.n4(x, y).into_iter().flatten() {
            let ni = self.idx(n.0, n.1);
            if self.grid[ni] == Metal && self.chance(n.0, n.1, 0x95, 15) {
                self.put(ni, Empty, 0);
                return false;
            }
        }
        false
    }

    fn step_plant(&mut self, x: usize, y: usize) {
        // Acid is handled by react_acid when the acid cell steps; plants only grow here.
        // Plant grows when adjacent to Water
        for n in self.n4(x, y) {
            let Some((nx, ny)) = n else { continue };
            let ni = self.idx(nx, ny);
            if self.grid[ni] == Water && self.chance(nx, ny, 0xA2, 25) {
                // find an empty neighbor to grow into
                for nn in self.n4(nx, ny) {
                    let Some((nnx, nny)) = nn else { continue };
                    let nni = self.idx(nnx, nny);
                    if self.grid[nni] == Empty && !(nnx == nx && nny == ny) {
                        self.put(nni, Plant, 0);
                        self.put(ni, Empty, 0);
                        return;
                    }
                }
            }
        }
    }

    fn step_ice(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);

        // Temperature-driven melt (contact heat soaks into temp / effective_temp).
        // effective_temp already includes hot solids/liquids via stored temp.
        let heat = self.effective_temp(x, y);
        if heat > 0 {
            let chance = ((heat as u32).min(400) / 2).max(50);
            if self.chance(x, y, 0xB0, chance) {
                // Near-freezing meltwater, not ambient — preserves cold continuity
                // without immediately re-freezing from the ice heat-source temp.
                self.put(i, Water, 0);
                self.temp[i] = 0;
                return;
            }
        }

        // Salt brine is handled when salt steps; acid etching when acid steps.
    }

    fn react_water(&mut self, x: usize, y: usize) -> bool {
        let i = self.idx(x, y);
        // freeze when very cold (LN2 diffusion / contact)
        if self.effective_temp(x, y) < 0 && self.chance(x, y, 0x81, 200) {
            self.put(i, Ice, 0);
            return true;
        }
        // Temperature-driven boil: heat soak is enough; contact with fire/lava
        // is no longer required (those paths still help via effective_temp).
        let heat = self.effective_temp(x, y);
        if heat >= 100 {
            // Base chance is high enough that sustained boiling-point water
            // converts within a few dozen ticks even after ambient bleed.
            let chance = (((heat as u32) - 100).min(500) / 2 + 150).min(600);
            if self.chance(x, y, 0x83, chance) {
                self.put(i, Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
                return true;
            }
        }
        for n in self.n4(x, y) {
            let Some((nx, ny)) = n else {
                continue;
            };
            let ni = self.idx(nx, ny);
            match self.grid[ni] {
                Fire if self.chance(nx, ny, 0x80, 300) => {
                    // do not flash-boil next to oil fires as readily
                    let oily = self
                        .n8(nx, ny)
                        .into_iter()
                        .flatten()
                        .any(|(ox, oy)| self.grid[self.idx(ox, oy)].is_oily());
                    if oily && !self.chance(nx, ny, 0x82, 100) {
                        continue;
                    }
                    self.put(i, Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
                    return true;
                }
                Lava => {
                    // water side of lava+water: boil; lava solidifies when it steps
                    self.put(i, Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
                    return true;
                }
                // Hot glass quenched by cold water shatters (thermal shock).
                Glass if self.temp[ni] >= 300 => {
                    self.put(ni, Sand, 0);
                }
                _ => {}
            }
        }
        false
    }

    /// Emit a continuous column of water beneath the faucet. The cell stays
    /// active every tick so the stream does not die when the world settles.
    fn step_faucet(&mut self, x: usize, y: usize) {
        let Some((tx, ty)) = self.adj(x, y, 0, 1) else {
            return;
        };
        let ti = self.idx(tx, ty);
        match self.grid[ti] {
            Empty => {
                self.put(ti, Water, 0);
            }
            // Keep pressure on an existing stream so gaps refill every tick.
            Water => {
                self.activate_next(tx, ty);
                // If the water immediately below has room further down, nudge it
                // so the faucet outlet does not stall as a single hanging drop.
                if let Some((bx, by)) = self.adj(tx, ty, 0, 1) {
                    let bi = self.idx(bx, by);
                    if self.grid[bi].is_empty() {
                        self.swap(ti, bi);
                        self.put(ti, Water, 0);
                    }
                }
            }
            _ => {}
        }
    }

    fn step_drain(&mut self, x: usize, y: usize) {
        for n in self.n8(x, y) {
            if let Some((nx, ny)) = n {
                let ni = self.idx(nx, ny);
                let m = self.grid[ni];
                if m.is_empty() || m == Drain {
                    continue;
                }
                if m.is_fluid() || m.is_gas() {
                    self.put(ni, Empty, 0);
                    return;
                }
            }
        }
    }
}

fn rand_life(m: Material) -> u16 {
    match m {
        Fire => rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX),
        Ember => rand_range(EMBER_LIFE_MIN, EMBER_LIFE_MAX),
        Steam => rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX),
        Smoke => rand_range(SMOKE_LIFE_MIN, SMOKE_LIFE_MAX),
        _ => 0,
    }
}

fn rand_range(min: u16, max: u16) -> u16 {
    rand::thread_rng().gen_range(min..=max)
}

fn fill_rect(world: &mut World, x0: usize, y0: usize, x1: usize, y1: usize, m: Material) {
    let x1 = x1.min(world.width);
    let y1 = y1.min(world.height);
    for y in y0.min(world.height)..y1 {
        for x in x0.min(world.width)..x1 {
            world.paint(x, y, m);
        }
    }
}

/// A two-storey timber house cut through its rooms, attic, and basement.
fn seed_house(world: &mut World) {
    let (w, h) = (world.width, world.height);
    if w < 48 || h < 30 {
        return;
    }

    let ground = h - 3;
    let left = w / 7;
    let right = w * 6 / 7;
    let basement = ground - 6;
    let first_floor = ground - 13;
    let attic_floor = ground - 22;
    let roof_peak = attic_floor.saturating_sub(8);
    let center = (left + right) / 2;

    fill_rect(world, 0, ground, w, h, Stone);
    fill_rect(world, left - 3, basement, right + 3, ground, Concrete);
    fill_rect(world, left - 1, basement + 2, right + 1, ground, Empty);

    // Concrete foundation, timber frame, and floor joists expose the structure.
    fill_rect(world, left, attic_floor, left + 2, basement, Wood);
    fill_rect(world, right - 2, attic_floor, right, basement, Wood);
    fill_rect(world, center - 1, attic_floor, center + 1, basement, Wood);
    fill_rect(world, left, first_floor, right, first_floor + 1, Wood);
    fill_rect(world, left, basement - 1, right, basement, Wood);
    fill_rect(world, left, attic_floor, right, attic_floor + 1, Wood);

    // Gabled roof, attic framing, brick chimney, and glazed windows.
    for x in left - 3..right + 3 {
        let distance = x.abs_diff(center);
        let y = roof_peak + distance * (attic_floor - roof_peak) / (right - center + 2);
        fill_rect(world, x, y, x + 1, y + 2, Wood);
    }
    let chimney = right - 8;
    fill_rect(
        world,
        chimney,
        roof_peak.saturating_sub(2),
        chimney + 4,
        first_floor,
        Stone,
    );
    fill_rect(
        world,
        chimney + 1,
        roof_peak.saturating_sub(1),
        chimney + 3,
        first_floor - 3,
        Empty,
    );
    fill_rect(
        world,
        chimney - 1,
        first_floor - 4,
        chimney + 5,
        first_floor,
        Stone,
    );
    world.paint(chimney + 2, first_floor - 2, Ember);
    for x in [left + 5, center - 7, center + 5, right - 7] {
        fill_rect(world, x, attic_floor + 3, x + 3, attic_floor + 6, Glass);
        fill_rect(world, x, first_floor + 3, x + 3, first_floor + 7, Glass);
    }

    // Room partitions, furnishings, stairs, plumbing, and a basement cistern.
    fill_rect(
        world,
        left + 10,
        attic_floor + 1,
        left + 11,
        first_floor,
        Wood,
    );
    fill_rect(
        world,
        center + 8,
        attic_floor + 1,
        center + 9,
        first_floor,
        Wood,
    );
    fill_rect(
        world,
        left + 4,
        first_floor - 3,
        left + 10,
        first_floor - 1,
        Wood,
    );
    fill_rect(
        world,
        center + 3,
        basement - 4,
        center + 10,
        basement - 2,
        Wood,
    );
    for step in 0..7 {
        fill_rect(
            world,
            center - 6 + step,
            first_floor + step,
            center - 4 + step,
            first_floor + step + 1,
            Wood,
        );
    }
    fill_rect(
        world,
        left + 4,
        basement + 1,
        left + 13,
        basement + 2,
        Metal,
    );
    fill_rect(world, left + 4, basement + 1, left + 5, ground - 1, Metal);
    fill_rect(world, left + 5, basement + 2, left + 12, ground - 1, Water);
    fill_rect(world, right - 8, basement + 2, right - 4, ground - 1, Oil);
}

/// A reinforced high-rise cut through offices, elevator core, services, and roof tank.
fn seed_skyscraper(world: &mut World) {
    let (w, h) = (world.width, world.height);
    if w < 48 || h < 30 {
        return;
    }

    let ground = h - 3;
    let left = w / 5;
    let right = w * 4 / 5;
    let top = 5;
    let core_left = w / 2 - 3;
    let core_right = w / 2 + 3;
    let floor_height = ((ground - top) / 7).max(3);

    fill_rect(world, 0, ground, w, h, Stone);
    fill_rect(world, left - 2, ground - 5, right + 2, ground, Concrete);
    fill_rect(world, left, top, left + 2, ground, Concrete);
    fill_rect(world, right - 2, top, right, ground, Concrete);
    fill_rect(world, core_left, top, core_left + 2, ground, Concrete);
    fill_rect(world, core_right - 2, top, core_right, ground, Concrete);
    fill_rect(
        world,
        core_left + 2,
        top + 2,
        core_right - 2,
        ground - 1,
        Empty,
    );

    for floor in (top + floor_height..ground).step_by(floor_height) {
        fill_rect(world, left, floor, right, floor + 1, Concrete);
        fill_rect(world, left + 3, floor - 1, core_left - 2, floor, Wood);
        fill_rect(world, core_right + 2, floor - 1, right - 3, floor, Wood);
        fill_rect(
            world,
            left + 5,
            floor - floor_height + 2,
            left + 6,
            floor,
            Wood,
        );
        fill_rect(
            world,
            right - 6,
            floor - floor_height + 2,
            right - 5,
            floor,
            Wood,
        );
        fill_rect(
            world,
            left + 2,
            floor - floor_height + 3,
            left + 3,
            floor - 2,
            Glass,
        );
        fill_rect(
            world,
            right - 3,
            floor - floor_height + 3,
            right - 2,
            floor - 2,
            Glass,
        );
    }

    // Metal elevator car and service risers show the building's vertical systems.
    fill_rect(
        world,
        core_left + 2,
        ground - floor_height - 2,
        core_right - 2,
        ground - floor_height + 2,
        Metal,
    );
    fill_rect(world, right - 7, top + 2, right - 6, ground - 2, Metal);
    fill_rect(world, right - 6, top + 2, right - 5, ground - 2, Water);
    fill_rect(world, left + 4, ground - 4, core_left - 2, ground - 2, Oil);

    // Roof tank sits above the concrete core, with a visible steel enclosure.
    let tank_left = core_left - 5;
    let tank_right = core_right + 5;
    fill_rect(world, tank_left, 0, tank_right, 1, Metal);
    fill_rect(world, tank_left, 0, tank_left + 1, top + 2, Metal);
    fill_rect(world, tank_right - 1, 0, tank_right, top + 2, Metal);
    fill_rect(world, tank_left + 1, 1, tank_right - 1, top + 1, Water);
    fill_rect(world, tank_left, top + 1, tank_right, top + 2, Metal);
}
