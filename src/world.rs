use rand::Rng;

use crate::material::Material;
use crate::scene_manager::SceneState;
use Material::*;

/// A loadable scene preset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    Blank,
    House,
    Volcano,
    Aquarium,
    Foundry,
    Rainstorm,
}

impl Scene {
    pub const ALL: [Scene; 6] = [
        Scene::Blank,
        Scene::House,
        Scene::Volcano,
        Scene::Aquarium,
        Scene::Foundry,
        Scene::Rainstorm,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Scene::Blank => "Blank",
            Scene::House => "House",
            Scene::Volcano => "Volcano",
            Scene::Aquarium => "Aquarium",
            Scene::Foundry => "Foundry",
            Scene::Rainstorm => "Rainstorm",
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
const FIRE_LIFE_MIN: u16 = 30;
const FIRE_LIFE_MAX: u16 = 70;
const EMBER_LIFE_MIN: u16 = 80;
const EMBER_LIFE_MAX: u16 = 170;
const STEAM_LIFE_MIN: u16 = 120;
const STEAM_LIFE_MAX: u16 = 280;
const SMOKE_LIFE_MIN: u16 = 80;
const SMOKE_LIFE_MAX: u16 = 180;

const CHUNK_W: usize = 64;
const CHUNK_H: usize = 32;

/// Fraction of cooled embers that leave a residue of ash; the rest are fully
/// consumed by the burn.
const ASH_CHANCE_PER_MILLE: u32 = 50;

/// How far a liquid tries to flow sideways in one tick. Bigger = flatter water;
/// lava stays viscous.
fn spread_of(m: Material) -> usize {
    match m {
        Water => 6,
        Acid => 4,
        Oil => 4,
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
            moved_tick: vec![u64::MAX; n],
            active_chunks: vec![false; chunks_len(width, height)],
            next_active_chunks: vec![false; chunks_len(width, height)],
            wood_seen: vec![false; n],
            chunks_x: chunks_x(width),
            chunks_y: chunks_y(height),
            tick: 0,
            scene: Scene::Blank,
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

    /// Paint a single cell (used by the brush; does not touch movement bookkeeping).
    pub fn paint(&mut self, x: usize, y: usize, m: Material) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = self.idx(x, y);
        self.grid[i] = m;
        self.seed[i] = rand::random();
        self.life[i] = rand_life(m);
        self.activate_now(x, y);
    }

    pub fn clear(&mut self) {
        for i in 0..self.grid.len() {
            self.grid[i] = Empty;
            self.life[i] = 0;
            self.seed[i] = rand::random();
        }
        self.tick = 0;
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

    pub fn load_scene(&mut self, scene: Scene) {
        self.clear();
        self.scene = scene;
        match scene {
            Scene::Blank => {}
            Scene::House => seed_house(self),
            Scene::Volcano => seed_volcano(self),
            Scene::Aquarium => seed_aquarium(self),
            Scene::Foundry => seed_foundry(self),
            Scene::Rainstorm => seed_rainstorm(self),
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
        let cw = width.min(self.width);
        let ch = height.min(self.height);
        for y in 0..ch {
            for x in 0..cw {
                grid[y * width + x] = self.grid[y * self.width + x];
                life[y * width + x] = self.life[y * self.width + x];
                seed[y * width + x] = self.seed[y * self.width + x];
            }
        }
        self.width = width;
        self.height = height;
        self.grid = grid;
        self.life = life;
        self.seed = seed;
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
        self.grid[i] = m;
        self.life[i] = life;
        self.seed[i] = rand::random();
        self.moved_tick[i] = self.tick;
        self.activate_idx(i);
    }

    fn swap(&mut self, a: usize, b: usize) {
        self.grid.swap(a, b);
        self.life.swap(a, b);
        self.seed.swap(a, b);
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
        m.is_empty()
            || matches!(m, Fire | Steam | Smoke)
            || (m.is_fluid() && Wood.density() > m.density())
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
            moves.push((i, ti, self.life[i], self.seed[i]));
            if !in_component[ti] && self.grid[ti] != Empty {
                displaced.push((self.grid[ti], self.life[ti], self.seed[ti]));
            }
        }

        for &(i, _, _, _) in &moves {
            self.grid[i] = Empty;
            self.life[i] = 0;
            self.moved_tick[i] = self.tick;
            self.activate_idx(i);
        }
        for &(_, ti, life, seed) in &moves {
            self.grid[ti] = Wood;
            self.life[ti] = life;
            self.seed[ti] = seed;
            self.moved_tick[ti] = self.tick;
            self.activate_idx(ti);
        }
        for &(i, _, _, _) in &moves {
            if target_set[i] {
                continue;
            }
            if let Some((m, life, seed)) = displaced.pop() {
                self.grid[i] = m;
                self.life[i] = life;
                self.seed[i] = seed;
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
                        match self.grid[i] {
                            Empty | Stone | Wood => continue,
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
                            Sand | Ash | Salt | Gunpowder => self.step_powder(x, y),
                            Water | Oil => self.step_liquid(x, y),
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
                            Mercury => self.step_liquid(x, y),
                        }
                    }
                }
            }
        }
        std::mem::swap(&mut self.active_chunks, &mut self.next_active_chunks);
        self.tick = self.tick.wrapping_add(1);
    }

    fn step_powder(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);
        let m = self.grid[i];

        // Salt dissolves in Water
        if m == Salt {
            for n in self.n4(x, y) {
                let Some((nx, ny)) = n else { continue };
                let ni = self.idx(nx, ny);
                if self.grid[ni] == Water && self.chance(nx, ny, 0x90, 80) {
                    self.put(ni, Water, 0);
                    if self.chance(nx, ny, 0x91, 400) {
                        self.put(i, Empty, 0);
                        return;
                    }
                }
            }
        }

        // Gunpowder explodes near heat
        if m == Gunpowder {
            for n in self.n8(x, y) {
                let Some((nx, ny)) = n else { continue };
                let ni = self.idx(nx, ny);
                let other = self.grid[ni];
                if other == Fire || other == Lava || other == Ember {
                    // propagate explosion to adjacent gunpowder
                    for nn in self.n8(nx, ny) {
                        let Some((nnx, nny)) = nn else { continue };
                        let nni = self.idx(nnx, nny);
                        if self.grid[nni] == Gunpowder && self.moved_tick[nni] != self.tick {
                            let r = self.roll(nnx, nny, 0x92);
                            if r < 700 {
                                self.put(nni, Fire, rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX));
                            } else if r < 900 {
                                self.put(nni, Smoke, rand_range(SMOKE_LIFE_MIN, SMOKE_LIFE_MAX));
                            }
                        }
                    }
                    self.put(i, Smoke, rand_range(SMOKE_LIFE_MIN / 2, SMOKE_LIFE_MAX / 2));
                    // spawn fire at the heat source neighbor
                    self.activate_next(nx, ny);
                    return;
                }
            }
        }

        let sink = |t: Material| t.is_empty() || (t.is_fluid() && m.density() > t.density());
        // Downward
        if let Some((tx, ty)) = self.adj(x, y, 0, 1) {
            let ti = self.idx(tx, ty);
            if sink(self.grid[ti]) {
                self.swap(i, ti);
                return;
            }
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
        let m = self.grid[i];

        // reactions first (may consume this cell)
        let consumed = match m {
            Lava => self.react_lava(x, y),
            Acid => self.react_acid(x, y),
            Water => self.react_water(x, y),
            _ => false,
        };
        if consumed {
            return;
        }
        let m = self.grid[self.idx(x, y)];
        if !m.is_liquid() {
            return;
        }

        // lava is viscous: only flows every other tick
        if m == Lava && !self.tick.is_multiple_of(2) {
            return;
        }

        let sink = |t: Material| t.is_empty() || (t.is_fluid() && m.density() > t.density());
        let rise = |t: Material| t.is_fluid() && m.density() < t.density();

        // Downward
        if let Some((tx, ty)) = self.adj(x, y, 0, 1) {
            let ti = self.idx(tx, ty);
            if sink(self.grid[ti]) {
                self.swap(i, ti);
                return;
            }
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
        // buoyancy: lighter liquid rises through a denser one
        if self.try_step(x, y, 0, -1, rise) {
            return;
        }
        // horizontal flow: travel several cells so a liquid levels out quickly
        // instead of piling into a thin column.
        let spread = spread_of(m);
        let empty = |t: Material| t.is_empty();
        let (d1, d2) = self.dirs(x, y, 0x21);
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
        }
        cx != x as i32
    }

    fn step_gas(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);
        let m = self.grid[i];
        let life = self.life[i].saturating_sub(1);
        if life == 0 {
            self.put(i, Empty, 0);
            return;
        }
        self.life[i] = life;

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

        let mut extinguished = false;
        for n in self.n8(x, y) {
            let Some((nx, ny)) = n else {
                continue;
            };
            let ni = self.idx(nx, ny);
            let other = self.grid[ni];
            if other == Water {
                // water quenches fire: both turn to steam
                self.put(ni, Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
                extinguished = true;
            } else if other.flammable() && self.moved_tick[ni] != self.tick {
                // wood smolders into glowing embers; oil flashes straight to flame
                if other == Wood && self.chance(nx, ny, 0x41, 120) {
                    self.put(ni, Ember, rand_range(EMBER_LIFE_MIN, EMBER_LIFE_MAX));
                } else if other == Oil && self.chance(nx, ny, 0x42, 450) {
                    self.put(ni, Fire, rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX));
                }
            }
        }

        if extinguished {
            self.put(i, Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
            return;
        }

        // a little smoke wafts up off the flame
        if self.chance(x, y, 0x43, 50)
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
        let empty = |t: Material| t.is_empty();
        // prefer sideways drift over straight-up so fire runs along surfaces
        let (d1, d2) = self.dirs(x, y, 0x40);
        if self.try_step(x, y, d1, 0, empty) {
            return;
        }
        if self.try_step(x, y, d2, 0, empty) {
            return;
        }
        if self.try_step(x, y, 0, -1, empty) {
            return;
        }
        let _ = self.try_step(x, y, d1, -1, empty);
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
                Wood if self.moved_tick[ni] != self.tick && self.chance(nx, ny, 0x51, 50) => {
                    self.put(ni, Ember, rand_range(EMBER_LIFE_MIN, EMBER_LIFE_MAX));
                }
                Oil if self.moved_tick[ni] != self.tick && self.chance(nx, ny, 0x52, 300) => {
                    self.put(ni, Fire, rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX));
                }
                Empty if ny < y => {
                    // flames lick upward, with a wisp of smoke now and then
                    let r = self.roll(nx, ny, 0x53);
                    if r < 80 {
                        self.put(ni, Fire, rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX));
                    } else if r < 130 {
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

        // settle slowly like a heavy grit
        if self.chance(x, y, 0x54, 600) {
            let sink =
                |t: Material| t.is_empty() || (t.is_fluid() && Ember.density() > t.density());
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
                Wood if self.moved_tick[ni] != self.tick && self.chance(nx, ny, 0x61, 400) => {
                    self.put(ni, Ember, rand_range(EMBER_LIFE_MIN, EMBER_LIFE_MAX));
                }
                Oil if self.moved_tick[ni] != self.tick && self.chance(nx, ny, 0x62, 500) => {
                    self.put(ni, Fire, rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX));
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
            if other.is_empty() || other == Acid || other == Stone {
                continue;
            }
            if self.chance(nx, ny, 0x70, 200) {
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

    /// Water next to fire has a chance to flash into steam (symmetric to fire's logic).
    fn step_plant(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);

        // Acid dissolves Plant
        for n in self.n4(x, y) {
            let Some((nx, ny)) = n else { continue };
            let ni = self.idx(nx, ny);
            if self.grid[ni] == Acid && self.chance(nx, ny, 0xA0, 200) {
                self.put(ni, Empty, 0);
                if self.chance(x, y, 0xA1, 350) {
                    self.put(i, Empty, 0);
                    return;
                }
            }
        }

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

        // Ice melts near Fire / Lava / Ember
        for n in self.n8(x, y) {
            let Some((nx, ny)) = n else { continue };
            let ni = self.idx(nx, ny);
            match self.grid[ni] {
                Fire | Lava | Ember if self.chance(nx, ny, 0xB0, 200) => {
                    self.put(i, Water, 0);
                    return;
                }
                _ => {}
            }
        }

        // Acid shatters Ice
        for n in self.n4(x, y) {
            let Some((nx, ny)) = n else { continue };
            let ni = self.idx(nx, ny);
            if self.grid[ni] == Acid && self.chance(nx, ny, 0xB1, 200) {
                self.put(ni, Empty, 0);
                if self.chance(x, y, 0xB2, 350) {
                    self.put(i, Empty, 0);
                    return;
                }
            }
        }
    }

    fn react_water(&mut self, x: usize, y: usize) -> bool {
        for n in self.n4(x, y) {
            let Some((nx, ny)) = n else {
                continue;
            };
            let ni = self.idx(nx, ny);
            if self.grid[ni] == Fire && self.chance(nx, ny, 0x80, 300) {
                self.put(
                    self.idx(x, y),
                    Steam,
                    rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX),
                );
                return true;
            }
        }
        false
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

fn fill_disc(world: &mut World, cx: isize, cy: isize, r: isize, m: Material) {
    let r2 = r * r;
    for y in cy - r..=cy + r {
        for x in cx - r..=cx + r {
            if x >= 0 && y >= 0 && x < world.width as isize && y < world.height as isize {
                let dx = x - cx;
                let dy = y - cy;
                if dx * dx + dy * dy <= r2 {
                    world.paint(x as usize, y as usize, m);
                }
            }
        }
    }
}

/// A detailed three-story house built to showcase structural collapse, fire,
/// fluid containment, and material reactions.
fn seed_house(world: &mut World) {
    let w = world.width;
    let h = world.height;
    if w < 40 || h < 24 {
        return;
    }

    let ground = h - 2;
    let left = w / 8;
    let right = w * 7 / 8;
    let center = (left + right) / 2;
    let floor_h = ((ground - 8) / 3).max(5);
    let bottom = ground - 2;
    let top = bottom - floor_h * 3;

    // Ground, basement slab, and a stone foundation keep the wood frame stable.
    fill_rect(world, 0, ground, w, h, Stone);
    fill_rect(world, left - 2, bottom, right + 2, ground, Stone);
    fill_rect(world, left, top, left + 2, bottom, Wood);
    fill_rect(world, right - 2, top, right, bottom, Wood);

    // Three independently furnished stories with continuous structural floors.
    for story in 0..=3 {
        let y = bottom - story * floor_h;
        fill_rect(world, left, y, right, y + 1, Wood);
    }
    for &x in &[left + 2, center, right - 3] {
        fill_rect(world, x, top, x + 1, bottom, Wood);
    }

    // Interior room dividers, each with a doorway.
    for story in 0..3 {
        let ceiling = bottom - (story + 1) * floor_h;
        let floor = bottom - story * floor_h;
        for &x in &[left + (right - left) / 3, left + (right - left) * 2 / 3] {
            fill_rect(world, x, ceiling + 1, x + 1, floor, Wood);
            fill_rect(world, x, floor - 3, x + 1, floor, Empty);
        }
    }

    // Front door and paired windows on every level.
    fill_rect(world, center - 2, bottom - 5, center + 3, bottom, Empty);
    for story in 0..3 {
        let floor = bottom - story * floor_h;
        let window_top = floor.saturating_sub(5);
        for &x in &[left, right - 1] {
            fill_rect(world, x, window_top, x + 1, floor - 2, Empty);
        }
    }

    // Alternating stair flights link all floors without blocking the rooms.
    for story in 0..3 {
        let floor = bottom - story * floor_h;
        let stair_left = if story.is_multiple_of(2) {
            center + 3
        } else {
            center.saturating_sub(floor_h + 4)
        };
        for step in 0..floor_h.saturating_sub(1) {
            let x = stair_left + step;
            let y = floor - 1 - step;
            if x < right - 3 && y > top {
                world.paint(x, y, Wood);
                world.paint(x, y + 1, Wood);
            }
        }
    }

    // Pitched stone roof protects the frame while leaving a usable attic.
    let roof_h = top.saturating_sub(1).min((right - left) / 5).max(3);
    for rise in 0..=roof_h {
        let y = top - rise;
        let inset = rise * (right - left) / 2 / roof_h;
        let lx = left.saturating_sub(3) + inset;
        let rx = (right + 3).saturating_sub(inset).min(w - 1);
        fill_rect(world, lx, y, (lx + 2).min(w), y + 1, Stone);
        fill_rect(
            world,
            rx.saturating_sub(1),
            y,
            (rx + 1).min(w),
            y + 1,
            Stone,
        );
    }

    // Rooftop cistern: water stays contained until its floor is damaged.
    let tank_left = left + 5;
    fill_rect(world, tank_left, top - 5, tank_left + 9, top - 1, Stone);
    fill_rect(world, tank_left + 1, top - 4, tank_left + 8, top - 2, Water);

    // Stone fireplace and chimney isolate embers from the wooden floors.
    let hearth = right - 9;
    fill_rect(world, hearth, bottom - 6, hearth + 6, bottom, Stone);
    fill_rect(world, hearth + 1, bottom - 4, hearth + 5, bottom - 1, Empty);
    fill_rect(world, hearth + 2, bottom - 3, hearth + 4, bottom - 1, Ember);
    fill_rect(
        world,
        hearth + 2,
        top.saturating_sub(6),
        hearth + 4,
        bottom - 6,
        Stone,
    );

    // Oil boiler in a stone utility room creates a deliberate fire hazard.
    fill_rect(world, left + 4, bottom - 6, left + 12, bottom, Stone);
    fill_rect(world, left + 5, bottom - 5, left + 11, bottom - 2, Oil);
    world.paint(left + 11, bottom - 2, Fire);

    // Upper-story planter reacts with water leaking from the cistern.
    fill_rect(
        world,
        center - 8,
        top + floor_h - 3,
        center + 8,
        top + floor_h - 2,
        Stone,
    );
    fill_rect(
        world,
        center - 7,
        top + floor_h - 5,
        center + 7,
        top + floor_h - 3,
        Plant,
    );
}

/// Stable stone volcano with lava channels aimed at water and oil basins.
fn seed_volcano(world: &mut World) {
    let w = world.width;
    let h = world.height;
    if w < 32 || h < 20 {
        return;
    }
    let ground = h - 2;
    let cx = w / 2;
    let summit = h / 4;
    fill_rect(world, 0, ground, w, h, Stone);

    for y in summit..ground {
        let half = 3 + (y - summit) * (w / 3) / (ground - summit);
        fill_rect(world, cx - half, y, cx + half + 1, y + 1, Stone);
        let throat = 1 + (y - summit) / 8;
        fill_rect(world, cx - throat, y, cx + throat + 1, y + 1, Lava);
    }
    fill_rect(world, cx - 4, summit - 1, cx + 5, summit + 2, Lava);

    // Open stone chutes carry lava down both slopes.
    for step in 0..ground - summit - 2 {
        let y = summit + 2 + step;
        let offset = 3 + step * (w / 3 - 4) / (ground - summit - 2);
        world.paint(cx.saturating_sub(offset), y, Lava);
        world.paint((cx + offset).min(w - 1), y, Lava);
    }

    // Left lava makes steam and stone; right lava ignites oil and wood.
    fill_rect(world, 2, ground - 5, w / 4, ground, Stone);
    fill_rect(world, 3, ground - 4, w / 4 - 1, ground - 1, Water);
    fill_rect(world, w * 3 / 4, ground - 5, w - 2, ground, Stone);
    fill_rect(world, w * 3 / 4 + 1, ground - 4, w - 3, ground - 2, Oil);
    fill_rect(world, w * 3 / 4 + 2, ground - 7, w - 4, ground - 6, Wood);
    fill_disc(world, cx as isize, summit as isize - 4, 2, Smoke);
}

/// Open aquarium that demonstrates density, dissolution, and sediment layers.
fn seed_aquarium(world: &mut World) {
    let w = world.width;
    let h = world.height;
    if w < 30 || h < 18 {
        return;
    }
    let left = 3;
    let right = w - 3;
    let top = h / 5;
    let bottom = h - 3;
    fill_rect(world, left, top, left + 1, bottom + 1, Stone);
    fill_rect(world, right - 1, top, right, bottom + 1, Stone);
    fill_rect(world, left, bottom, right, bottom + 1, Stone);
    fill_rect(world, left + 1, top + 2, right - 1, bottom, Water);

    // Sand and heavy mercury settle; oil rises; salt dissolves into the water.
    for x in left + 1..right - 1 {
        let depth = 1 + (x * 5 % 3);
        fill_rect(world, x, bottom - depth, x + 1, bottom, Sand);
    }
    for i in 0..5 {
        fill_disc(
            world,
            (left + 5 + i * (right - left - 10) / 4) as isize,
            (bottom - 4 - i % 3) as isize,
            1,
            Oil,
        );
    }
    fill_rect(world, left + 4, top + 3, left + 8, top + 6, Salt);
    fill_rect(world, right - 9, top + 3, right - 5, top + 5, Mercury);

    // A stone shelf supports wood; erasing it drops the whole connected chunk.
    let shelf = w / 2;
    fill_rect(world, shelf - 6, bottom - 7, shelf + 7, bottom - 6, Stone);
    fill_rect(world, shelf - 5, bottom - 9, shelf + 6, bottom - 7, Wood);
    fill_rect(world, shelf - 1, bottom - 12, shelf + 2, bottom - 9, Plant);
}

/// Foundry with separated fuel, heat, coolant, and reactive feed hoppers.
fn seed_foundry(world: &mut World) {
    let w = world.width;
    let h = world.height;
    if w < 36 || h < 22 {
        return;
    }
    let ground = h - 2;
    let third = w / 3;
    fill_rect(world, 0, ground, w, h, Stone);

    // Three stone bays prevent every reaction from firing at once.
    for x in [1, third, third * 2, w - 2] {
        fill_rect(world, x, ground - 12, x + 1, ground, Stone);
    }
    fill_rect(world, 2, ground - 5, third, ground, Lava);
    fill_rect(world, third + 1, ground - 5, third * 2, ground, Water);
    fill_rect(world, third * 2 + 1, ground - 5, w - 2, ground, Oil);

    // Powder hoppers fall into each bay when their wooden plugs burn.
    let hopper_y = ground - 11;
    fill_rect(world, 3, hopper_y, third - 2, hopper_y + 3, Sand);
    fill_rect(
        world,
        third + 3,
        hopper_y,
        third * 2 - 2,
        hopper_y + 3,
        Salt,
    );
    fill_rect(
        world,
        third * 2 + 3,
        hopper_y,
        w - 4,
        hopper_y + 3,
        Gunpowder,
    );
    for (l, r) in [(2, third), (third + 1, third * 2), (third * 2 + 1, w - 2)] {
        fill_rect(world, l, hopper_y + 3, r, hopper_y + 4, Wood);
    }

    // A central furnace can breach both the water and oil bays.
    fill_rect(world, third - 1, ground - 9, third + 2, ground - 5, Stone);
    fill_rect(world, third, ground - 8, third + 1, ground - 5, Fire);
    fill_rect(
        world,
        third * 2 - 1,
        ground - 9,
        third * 2 + 2,
        ground - 5,
        Stone,
    );
    fill_rect(
        world,
        third * 2,
        ground - 8,
        third * 2 + 1,
        ground - 5,
        Ember,
    );
}

/// Watershed map with rain, porous sand, plant beds, and an oil spill.
fn seed_rainstorm(world: &mut World) {
    let w = world.width;
    let h = world.height;
    if w < 36 || h < 22 {
        return;
    }
    let ground = h - 2;
    fill_rect(world, 0, ground, w, h, Stone);

    // Sloped sand terrain drains into two stone-lined catch basins.
    for x in 0..w {
        let depth = 2 + x.abs_diff(w / 2) * 5 / w;
        fill_rect(world, x, ground - depth, x + 1, ground, Sand);
    }
    for &(l, r) in &[(2, w / 4), (w * 3 / 4, w - 2)] {
        fill_rect(world, l, ground - 7, l + 1, ground, Stone);
        fill_rect(world, r - 1, ground - 7, r, ground, Stone);
        fill_rect(world, l, ground - 1, r, ground, Stone);
    }

    // Water clouds immediately rain; steam pockets rise through them.
    for i in 0..4 {
        let cx = w * (i + 1) / 5;
        fill_disc(world, cx as isize, 4 + (i % 2) as isize, 3, Water);
        fill_disc(world, cx as isize + 3, 6, 1, Steam);
    }

    // Plants consume nearby water while an oil spill floats on the right basin.
    fill_rect(world, 3, ground - 9, w / 4 - 1, ground - 7, Plant);
    fill_rect(world, w * 3 / 4 + 1, ground - 6, w - 4, ground - 4, Oil);
    fill_rect(world, w * 3 / 4 + 3, ground - 9, w - 6, ground - 8, Wood);
    world.paint(w - 7, ground - 10, Fire);
}
