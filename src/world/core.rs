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

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

pub struct World {
    pub width: usize,
    pub height: usize,
    pub(super) grid: Vec<Material>,
    pub(super) life: Vec<u16>,
    pub(super) seed: Vec<u8>,
    /// Approximate Celsius temperature per cell.
    pub(super) temp: Vec<i16>,
    /// Scratch buffer for heat diffusion.
    pub(super) temp_next: Vec<i16>,
    pub(super) moved_tick: Vec<u64>,
    pub(super) active_chunks: Vec<bool>,
    pub(super) next_active_chunks: Vec<bool>,
    /// Scratch space for connected-component structural physics.
    pub(super) structural_seen: Vec<bool>,
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
            temp: vec![AMBIENT_TEMP; n],
            temp_next: vec![AMBIENT_TEMP; n],
            moved_tick: vec![u64::MAX; n],
            active_chunks: vec![false; chunks_len(width, height)],
            next_active_chunks: vec![false; chunks_len(width, height)],
            structural_seen: vec![false; n],
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
        self.structural_seen.fill(false);
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
        self.structural_seen = vec![false; width * height];
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

    /// Move downward through at most `steps` adjacent cells. Each intermediate
    /// cell is checked and swapped, preserving collision and displacement rules.
    pub(super) fn try_fall(
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
            let (tx, ty) = self
                .adj(x, y, dx, dy)
                .expect("prechecked structural translation");
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
            self.grid[ti] = material;
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
            self.moved_tick[i] = self.tick;
            self.activate_idx(i);
        }
    }

    /// Move unsupported connected structural islands down one cell. Glass turns
    /// into broken glass when an island that fell on the previous tick hits support.

    pub(super) fn step_structural_components(&mut self) {
        let n = self.grid.len();
        let mut stack = Vec::new();
        let mut component = Vec::new();
        let mut in_component = vec![false; n];

        for material in [Wood, Stone, Glass] {
            self.structural_seen.fill(false);
            for y in (0..self.height).rev() {
                for x in 0..self.width {
                    let start = self.idx(x, y);
                    if self.grid[start] != material
                        || self.moved_tick[start] == self.tick
                        || self.structural_seen[start]
                    {
                        continue;
                    }

                    stack.clear();
                    component.clear();
                    stack.push(start);
                    self.structural_seen[start] = true;
                    while let Some(i) = stack.pop() {
                        component.push(i);
                        let cx = i % self.width;
                        let cy = i / self.width;
                        for n in self.n4(cx, cy) {
                            let Some((nx, ny)) = n else { continue };
                            let ni = self.idx(nx, ny);
                            if self.grid[ni] == material
                                && self.moved_tick[ni] != self.tick
                                && !self.structural_seen[ni]
                            {
                                self.structural_seen[ni] = true;
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
                        if material.flammable() {
                            self.activate_next(x, y);
                            if self.step_combustible(x, y) {
                                continue;
                            }
                        } else if material != Ice
                            && material.melt().is_some_and(|(melt_temp, _, _)| {
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
