use rand::Rng;

use crate::material::Material;
use Material::*;

/// Lifetime bounds (in ticks).
const FIRE_LIFE_MIN: u16 = 30;
const FIRE_LIFE_MAX: u16 = 70;
const EMBER_LIFE_MIN: u16 = 80;
const EMBER_LIFE_MAX: u16 = 170;
const STEAM_LIFE_MIN: u16 = 120;
const STEAM_LIFE_MAX: u16 = 280;
const SMOKE_LIFE_MIN: u16 = 80;
const SMOKE_LIFE_MAX: u16 = 180;

/// Fraction of cooled embers that leave a residue of ash; the rest are fully
/// consumed by the burn.
const ASH_CHANCE: f32 = 0.05;

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

/// A falling-sand style cellular world.
pub struct World {
    pub width: usize,
    pub height: usize,
    grid: Vec<Material>,
    life: Vec<u16>,
    seed: Vec<u8>,
    moved: Vec<bool>,
    tick: u64,
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
            moved: vec![false; n],
            tick: 0,
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

    /// Paint a single cell (used by the brush; does not touch the `moved` flag).
    pub fn paint(&mut self, x: usize, y: usize, m: Material) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = self.idx(x, y);
        self.grid[i] = m;
        self.seed[i] = rand::random();
        self.life[i] = rand_life(m);
    }

    pub fn clear(&mut self) {
        for i in 0..self.grid.len() {
            self.grid[i] = Empty;
            self.life[i] = 0;
            self.seed[i] = rand::random();
        }
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
        self.moved = vec![false; width * height];
    }

    // --- internal mutation helpers (used only during a step) ---

    fn put(&mut self, i: usize, m: Material, life: u16) {
        self.grid[i] = m;
        self.life[i] = life;
        self.seed[i] = rand::random();
        self.moved[i] = true;
    }

    fn swap(&mut self, a: usize, b: usize) {
        self.grid.swap(a, b);
        self.life.swap(a, b);
        self.seed.swap(a, b);
        self.moved[a] = true;
        self.moved[b] = true;
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

    // --- the step ---

    pub fn step(&mut self) {
        self.moved.fill(false);
        let w = self.width;
        let h = self.height;

        for y in (0..h).rev() {
            let ltr = self.tick.wrapping_add(y as u64).is_multiple_of(2);
            for k in 0..w {
                let x = if ltr { k } else { w - 1 - k };
                let i = self.idx(x, y);
                if self.moved[i] {
                    continue;
                }
                match self.grid[i] {
                    Empty | Wall | Wood => continue,
                    Fire => self.step_fire(x, y),
                    Ember => self.step_ember(x, y),
                    Steam | Smoke => self.step_gas(x, y),
                    Sand | Ash => self.step_powder(x, y),
                    Water | Oil | Acid | Lava => self.step_liquid(x, y),
                }
            }
        }
        self.tick = self.tick.wrapping_add(1);
    }

    fn step_powder(&mut self, x: usize, y: usize) {
        let m = self.grid[self.idx(x, y)];
        let sink = |t: Material| t.is_empty() || (t.is_fluid() && m.density() > t.density());
        if self.try_step(x, y, 0, 1, sink) {
            return;
        }
        let (d1, d2) = if rand::random() { (-1, 1) } else { (1, -1) };
        if self.try_step(x, y, d1, 1, sink) {
            return;
        }
        let _ = self.try_step(x, y, d2, 1, sink);
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

        if self.try_step(x, y, 0, 1, sink) {
            return;
        }
        let (d1, d2) = if rand::random() { (-1, 1) } else { (1, -1) };
        if self.try_step(x, y, d1, 1, sink) {
            return;
        }
        if self.try_step(x, y, d2, 1, sink) {
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
        let (d1, d2) = if rand::random() { (-1, 1) } else { (1, -1) };
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
        let (d1, d2) = if rand::random() { (-1, 1) } else { (1, -1) };
        if self.try_step(x, y, d1, -1, rise) {
            return;
        }
        if self.try_step(x, y, d2, -1, rise) {
            return;
        }
        let empty = |t: Material| t.is_empty();
        let (d1, d2) = if rand::random() { (-1, 1) } else { (1, -1) };
        if self.try_step(x, y, d1, 0, empty) {
            return;
        }
        let _ = self.try_step(x, y, d2, 0, empty);

        // slow dissipation
        if rand::random::<f32>() < 0.004 {
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
            } else if other.flammable() && !self.moved[ni] {
                // wood smolders into glowing embers; oil flashes straight to flame
                if other == Wood && rand::random::<f32>() < 0.12 {
                    self.put(ni, Ember, rand_range(EMBER_LIFE_MIN, EMBER_LIFE_MAX));
                } else if other == Oil && rand::random::<f32>() < 0.45 {
                    self.put(ni, Fire, rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX));
                }
            }
        }

        if extinguished {
            self.put(i, Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
            return;
        }

        // a little smoke wafts up off the flame
        if rand::random::<f32>() < 0.05 {
            if let Some((ux, uy)) = self.adj(x, y, 0, -1) {
                let ui = self.idx(ux, uy);
                if self.grid[ui] == Empty {
                    self.put(ui, Smoke, rand_range(SMOKE_LIFE_MIN, SMOKE_LIFE_MAX));
                }
            }
        }

        // Rise like a hot gas, but linger ~40% of the time so it can keep
        // spreading along a fuel surface instead of floating straight up.
        if rand::random::<f32>() >= 0.6 {
            return;
        }
        let empty = |t: Material| t.is_empty();
        if self.try_step(x, y, 0, -1, empty) {
            return;
        }
        let (d1, d2) = if rand::random() { (-1, 1) } else { (1, -1) };
        if self.try_step(x, y, d1, -1, empty) {
            return;
        }
        let _ = self.try_step(x, y, d2, -1, empty);
    }

    /// Only `ASH_CHANCE` of cooled embers leave a residue of ash; the rest burn
    /// away completely.
    fn residue(&mut self, i: usize) {
        if rand::random::<f32>() < ASH_CHANCE {
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
                Wood if !self.moved[ni] && rand::random::<f32>() < 0.05 => {
                    self.put(ni, Ember, rand_range(EMBER_LIFE_MIN, EMBER_LIFE_MAX));
                }
                Oil if !self.moved[ni] && rand::random::<f32>() < 0.3 => {
                    self.put(ni, Fire, rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX));
                }
                Empty if ny < y => {
                    // flames lick upward, with a wisp of smoke now and then
                    let r = rand::random::<f32>();
                    if r < 0.08 {
                        self.put(ni, Fire, rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX));
                    } else if r < 0.13 {
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
        if rand::random::<f32>() < 0.6 {
            let sink = |t: Material| t.is_empty() || (t.is_fluid() && Ember.density() > t.density());
            if self.try_step(x, y, 0, 1, sink) {
                return;
            }
            let (d1, d2) = if rand::random() { (-1, 1) } else { (1, -1) };
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
                Wood if !self.moved[ni] && rand::random::<f32>() < 0.4 => {
                    self.put(ni, Ember, rand_range(EMBER_LIFE_MIN, EMBER_LIFE_MAX));
                }
                Oil if !self.moved[ni] && rand::random::<f32>() < 0.5 => {
                    self.put(ni, Fire, rand_range(FIRE_LIFE_MIN, FIRE_LIFE_MAX));
                }
                _ => {}
            }
        }
        if solidified {
            self.put(self.idx(x, y), Wall, 0);
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
            if other.is_empty() || other == Acid || other == Wall {
                continue;
            }
            if rand::random::<f32>() < 0.2 {
                self.put(ni, Empty, 0);
                if rand::random::<f32>() < 0.35 {
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
    fn react_water(&mut self, x: usize, y: usize) -> bool {
        for n in self.n4(x, y) {
            let Some((nx, ny)) = n else {
                continue;
            };
            let ni = self.idx(nx, ny);
            if self.grid[ni] == Fire && rand::random::<f32>() < 0.3 {
                self.put(self.idx(x, y), Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
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
