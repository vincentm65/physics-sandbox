use super::*;

impl World {
    pub(super) fn explode(&mut self, x: usize, y: usize, radius: i32) {
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
        cells.sort_unstable_by_key(|cell| std::cmp::Reverse(cell.0));

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

        // Atmosphere overpressure / heat from explosion.
        if self.atmos_enabled {
            self.explode_atmos_effect(x, y, radius);
        }
    }

    /// Push the cell at `(x, y)` further from the blast origin along `(dx, dy)`.
    /// Combines existing velocity with the outward displacement as an impulse,
    /// clamped through MAX_VELOCITY. Returns true when the grain left its original cell.
    pub(super) fn fling_outward(&mut self, x: usize, y: usize, dx: i32, dy: i32) -> bool {
        let sx = dx.signum();
        let sy = dy.signum();
        let i = self.idx(x, y);
        let material = self.grid[i];
        let life = self.life[i];
        let seed = self.seed[i];
        let temp = self.temp[i];
        let vx = self.vx[i];
        let vy = self.vy[i];
        let vy_frac = self.vy_frac[i];
        let y_frac = self.y_frac[i];
        let vx_frac = self.vx_frac[i];
        let x_frac = self.x_frac[i];

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
            self.vx[i] = 0;
            self.vy[i] = 0;
            self.vx_frac[i] = 0;
            self.x_frac[i] = 0;
            self.vy_frac[i] = 0;
            self.y_frac[i] = 0;
            self.temp[i] = AMBIENT_TEMP;
            self.moved_tick[i] = self.tick;
            self.activate_idx(i);

            // Combine existing velocity with outward impulse in fixed-point units.
            let new_vx = (vx as i32 + px).clamp(-(MAX_VELOCITY as i32), MAX_VELOCITY as i32) as i8;
            let max_fixed = MAX_VELOCITY as i16 * VELOCITY_SCALE as i16;
            let fixed_vy = vy as i16 * VELOCITY_SCALE as i16 + vy_frac as i16;
            let new_fixed_vy =
                (fixed_vy + py as i16 * VELOCITY_SCALE as i16).clamp(-max_fixed, max_fixed);
            let new_vy = (new_fixed_vy / VELOCITY_SCALE as i16) as i8;
            let new_vy_frac = (new_fixed_vy % VELOCITY_SCALE as i16) as i8;

            self.grid[li] = material;
            self.life[li] = life;
            self.seed[li] = seed;
            self.temp[li] = temp;
            self.vx[li] = new_vx;
            self.vy[li] = new_vy;
            self.vx_frac[li] = vx_frac;
            self.x_frac[li] = x_frac;
            self.vy_frac[li] = new_vy_frac;
            self.y_frac[li] = y_frac;
            self.moved_tick[li] = self.tick;
            self.activate_idx(li);
            return true;
        }
        false
    }

    pub(super) fn step_combustible(&mut self, x: usize, y: usize, effective_temp: i16) -> bool {
        let i = self.idx(x, y);
        let material = self.grid[i];
        let Some((ignition_temperature, ignition_delay, burn_life)) = material.combustion() else {
            return false;
        };
        let heat = effective_temp.max(0) as u16;

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
    pub(super) fn step_melt(&mut self, x: usize, y: usize, effective_temp: i16) -> bool {
        let i = self.idx(x, y);
        let material = self.grid[i];
        // Ice melting is handled in step_ice (contact + temperature).
        if material == Ice {
            return false;
        }
        let Some((melt_temp, delay, product)) = material.melt() else {
            return false;
        };
        let heat = effective_temp.max(0) as u16;
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

    pub(super) fn step_tnt(&mut self, x: usize, y: usize) {
        if self.is_heated(x, y) {
            self.explode(x, y, TNT_BLAST_RADIUS);
        }
    }

    pub(super) fn step_fuse(&mut self, x: usize, y: usize) {
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

    pub(super) fn is_heated(&self, x: usize, y: usize) -> bool {
        self.effective_temp(x, y) >= 600
    }

    pub(super) fn step_c4(&mut self, x: usize, y: usize) {
        self.activate_next(x, y);
        if self.is_heated(x, y) {
            self.explode(x, y, C4_BLAST_RADIUS);
        }
    }

    pub(super) fn step_firework(&mut self, x: usize, y: usize) {
        self.activate_next(x, y);
        let i = self.idx(x, y);
        if self.life[i] == 0 {
            if self.is_heated(x, y) {
                // A rocket's seed selects its ascent height, burst design, and colour.
                // Clear the low flame and fuse before bursting.
                self.life[i] = 32 + (self.seed[i] as u16 % 20);
                self.moved_tick[i] = self.tick;
            }
            return;
        }

        self.life[i] -= 1;
        if self.life[i] == 0 {
            self.firework_burst(x, y);
            return;
        }

        // Leave a short, bright trail so the launch reads as a moving rocket.
        if let Some((tx, ty)) = self.adj(x, y, 0, 1) {
            let ti = self.idx(tx, ty);
            if self.grid[ti].is_empty() || matches!(self.grid[ti], Smoke | Steam) {
                let colour = (self.seed[i] / 3) % 6;
                self.put(ti, FireworkSpark, 8 + (self.seed[i] as u16 % 5));
                self.seed[ti] = colour;
            }
        }

        let passable = |m: Material| m.is_empty() || m.is_gas();
        // Each rocket gets a stable, seeded ±11–18° course rather than rising
        // perfectly vertical. The occasional diagonal step approximates its angle.
        let dx = if self.seed[i].is_multiple_of(2) {
            -1
        } else {
            1
        };
        let diagonal_every = 3 + (self.seed[i] as u16 % 3);
        if self.life[i].is_multiple_of(diagonal_every) && self.try_step(x, y, dx, -1, passable) {
            return;
        }
        let _ = self.try_step(x, y, 0, -1, passable);
    }

    fn firework_burst(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);
        let seed = self.seed[i];
        let radius = 5 + (seed as i32 % 6);
        let radius2 = radius * radius;
        let design = seed % 3;
        let colour = (seed / 3) % 6;
        self.put(i, Empty, 0);

        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let dist2 = dx * dx + dy * dy;
                if dist2 == 0 || dist2 > radius2 {
                    continue;
                }
                let selected = match design {
                    // Ring, eight-point star, and dense chrysanthemum.
                    0 => (dist2 - radius2).abs() <= radius,
                    1 => dx == 0 || dy == 0 || dx.abs() == dy.abs(),
                    _ => dist2 >= radius2 / 3,
                };
                if !selected {
                    continue;
                }
                let Some((sx, sy)) = self.adj(x, y, dx, dy) else {
                    continue;
                };
                let si = self.idx(sx, sy);
                if !(self.grid[si].is_empty() || self.grid[si].is_gas()) {
                    continue;
                }
                // Outer sparks fade first, making the burst contract as it falls.
                self.put(si, FireworkSpark, 22 + (radius2 - dist2) as u16);
                self.seed[si] = colour;
            }
        }
    }

    pub(super) fn step_firework_spark(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);
        let life = self.life[i].saturating_sub(1);
        if life == 0 {
            self.put(i, Empty, 0);
            return;
        }
        self.life[i] = life;

        // Apply vertical force (gravity) after lifecycle checks.
        self.apply_vertical_force(i);

        // Velocity-driven movement (Phase B).
        if self.try_velocity_move(x, y) {
            return;
        }

        let passable = |m: Material| m.is_empty() || matches!(m, Smoke | Steam);
        let (d1, _) = self.dirs(x, y, 0x71);
        if self.chance(x, y, 0x72, 350) {
            let _ = self.try_step(x, y, d1, 1, passable);
        }
    }

    pub(super) fn step_powder(&mut self, x: usize, y: usize) {
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

        // Apply vertical force (gravity) after lifecycle/reaction checks.
        self.apply_vertical_force(i);
        let has_vertical_step = self.has_vertical_step(i);

        // --- Velocity-driven movement (Phase B) ---
        // If the cell has stored velocity, trace the line using integer
        // DDA so fast movement cannot tunnel through thin barriers.
        if self.try_velocity_move(x, y) {
            return;
        }
        if !has_vertical_step {
            return;
        }

        // Diagonal avalanche fallback after a downward collision.
        let sink = |other| m.can_sink_into(other);
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

    /// Convert the weight of a liquid column into lateral momentum at an open
    /// boundary. The cells remain incompressible; pressure only acts when the
    /// column is supported and has somewhere to discharge.
    fn apply_liquid_pressure(&mut self, x: usize, y: usize, m: Material) {
        let cap = pressure_speed_of(m);
        if cap == 0 {
            return;
        }

        let supported = self
            .adj(x, y, 0, 1)
            .is_none_or(|(bx, by)| !m.can_sink_into(self.get(bx, by)));
        if !supported {
            return;
        }

        let mut head = 1usize;
        let mut cy = y;
        while head < 16 && cy > 0 {
            cy -= 1;
            if !self.get(x, cy).is_liquid() {
                break;
            }
            head += 1;
        }
        if head < 2 {
            return;
        }

        let left_open = x > 0 && self.get(x - 1, y) == Empty;
        let right_open = x + 1 < self.width && self.get(x + 1, y) == Empty;
        let dir = match (left_open, right_open) {
            (true, false) => -1,
            (false, true) => 1,
            (true, true) => self.dirs(x, y, 0x22).0,
            (false, false) => return,
        };
        let speed = (1 + (head.saturating_sub(1) / 3) as i8).min(cap);
        let i = self.idx(x, y);
        if self.vx[i].unsigned_abs() < speed as u8 {
            self.vx[i] = dir as i8 * speed;
            self.vx_frac[i] = 0;
            self.x_frac[i] = 0;
            self.activate_idx(i);
        }
    }

    pub(super) fn step_liquid(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);

        // reactions first (may consume this cell)
        if self.react_cell(x, y) {
            return;
        }
        let m = self.grid[i];
        if !m.is_liquid() {
            return;
        }

        // Sticky/viscous checks must occur before velocity movement so
        // napalm/lava cadence remains effective.
        if matches!(m, Lava | Napalm) && !self.tick.is_multiple_of(2) {
            return;
        }
        if m.sticky()
            && let Some((bx, by)) = self.adj(x, y, 0, 1)
        {
            let below = self.grid[self.idx(bx, by)];
            if !below.is_empty() && !below.is_fluid() && !self.chance(x, y, 0x55, 80) {
                return;
            }
        }

        // Apply vertical force and hydrostatic pressure after lifecycle/reaction
        // checks. Supported columns discharge sideways with speed proportional
        // to their head; falling liquid remains governed by gravity.
        self.apply_vertical_force(i);
        self.apply_liquid_pressure(x, y, m);
        let has_vertical_step = self.has_vertical_step(i);

        // --- Velocity-driven movement (Phase B) ---
        if self.try_velocity_move(x, y) {
            return;
        }
        if !has_vertical_step {
            return;
        }

        let sink = |other| m.can_sink_into(other);
        let rise = |t: Material| t.is_fluid() && m.density() < t.density();

        // Diagonal collision fallback.
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
    pub(super) fn flow(
        &mut self,
        x: usize,
        y: usize,
        dir: i32,
        range: usize,
        allow: impl Fn(Material) -> bool,
    ) -> bool {
        let material = self.get(x, y);
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
        if cx == x as i32 {
            return false;
        }

        let momentum = flow_momentum_of(material);
        if momentum > 0 {
            let i = self.idx(cx as usize, y);
            self.vx[i] = dir as i8 * momentum;
            self.vx_frac[i] = 0;
            self.x_frac[i] = 0;
            self.activate_idx(i);
        }
        true
    }

    pub(super) fn step_gas(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);
        let m = self.grid[i];
        let life = self.life[i].saturating_sub(1);
        let on_cold_surface = |world: &World, x: usize, y: usize| {
            world.n4(x, y).into_iter().flatten().any(|(nx, ny)| {
                matches!(
                    world.grid[world.idx(nx, ny)],
                    Ice | LiquidNitrogen | Metal | Glass
                ) || (world.grid[world.idx(nx, ny)] == Water && world.temp[world.idx(nx, ny)] < 40)
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

        // --- Velocity-driven movement (Phase B) ---
        // Apply vertical force (buoyancy) after lifecycle/reaction checks.
        self.apply_vertical_force(i);
        let has_vertical_step = self.has_vertical_step(i);

        if self.try_velocity_move(x, y) {
            return;
        }
        if !has_vertical_step {
            return;
        }

        // Diagonal and horizontal ceiling-spread fallback.
        let d = m.density();
        let rise = |t: Material| t.is_empty() || (t.is_fluid() && d < t.density());
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

    pub(super) fn step_fire(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);
        let life = self.life[i].saturating_sub(1);
        if life == 0 {
            self.put(i, Empty, 0);
            return;
        }
        self.life[i] = life;

        // Atmosphere‑integrated oxygen check (when enabled).
        if self.atmos_enabled {
            // O₂ depletion + extinction is handled by step_combustion_atmos()
            // which runs before cellular stepping.  If the fire was already
            // converted to smoke there, return early.
            if self.grid[i] != Fire {
                return;
            }
        } else {
            // Legacy fallback: no free neighbour → smother.
            let mut has_air = false;
            for (nx, ny) in self.n8(x, y).into_iter().flatten() {
                let ni = self.idx(nx, ny);
                let other = self.grid[ni];
                has_air |= matches!(other, Empty | Smoke | Steam | Fire) || other.flammable();
            }
            if !has_air {
                self.put(i, Smoke, rand_range(SMOKE_LIFE_MIN / 2, SMOKE_LIFE_MAX / 2));
                return;
            }
        }

        let mut oily = false;
        let mut water = [0; 8];
        let mut water_len = 0;
        let mut extinguished = false;
        for (nx, ny) in self.n8(x, y).into_iter().flatten() {
            let ni = self.idx(nx, ny);
            let other = self.grid[ni];
            oily |= other.is_oily();
            if other == Water {
                water[water_len] = ni;
                water_len += 1;
            } else if other == LiquidNitrogen {
                extinguished = true;
            }
        }

        // Oil/napalm fires shrug off water: water boils away, flame keeps burning.
        for &ni in &water[..water_len] {
            self.put(ni, Steam, rand_range(STEAM_LIFE_MIN, STEAM_LIFE_MAX));
            if !oily {
                extinguished = true;
            }
            // greasy fires: water steams off without killing the flame
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

        self.apply_vertical_force(i);
        let has_vertical_step = self.has_vertical_step(i);
        if self.try_velocity_move(x, y) {
            return;
        }
        if !has_vertical_step {
            return;
        }

        let passable = |t: Material| t.is_empty() || t == Smoke;
        let (d1, d2) = self.dirs(x, y, 0x40);
        if self.try_step(x, y, d1, -1, passable) {
            return;
        }
        if self.try_step(x, y, d1, 0, passable) {
            return;
        }
        let _ = self.try_step(x, y, d2, 0, passable);
    }

    /// Only `ASH_CHANCE` of cooled embers leave a residue of ash; the rest burn
    /// away completely.
    pub(super) fn residue(&mut self, i: usize) {
        if self.chance_idx(i, 0x45, ASH_CHANCE_PER_MILLE) {
            self.put(i, Ash, 0);
        } else {
            self.put(i, Empty, 0);
        }
    }

    /// A smoldering coal: the actual burning core left behind when wood catches.
    /// It glows, ignites neighbours, licks flames upward, breathes smoke, and
    /// finally cools into ash. Water quenches it instantly.
    pub(super) fn step_ember(&mut self, x: usize, y: usize) {
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

        // Apply vertical force (gravity) after lifecycle checks.
        self.apply_vertical_force(i);

        // Velocity-driven movement (Phase B).
        if self.try_velocity_move(x, y) {
            return;
        }

        // Embers mostly burn out where they form instead of piling up as grit.
        if self.chance(x, y, 0x54, 100) {
            let sink = |other| Ember.can_sink_into(other);
            let (d1, d2) = self.dirs(x, y, 0x50);
            if self.try_step(x, y, d1, 1, sink) {
                return;
            }
            let _ = self.try_step(x, y, d2, 1, sink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fling_outward_carries_velocity_and_clears_source() {
        let mut world = World::new(7, 3);
        world.paint(2, 1, Water);
        world.set_velocity(2, 1, -3, 4);
        let source = world.idx(2, 1);
        world.vy_frac[source] = -2;
        world.y_frac[source] = 3;

        assert!(world.fling_outward(2, 1, 1, 0));
        assert_eq!(world.get(2, 1), Empty);
        assert_eq!(world.velocity_at(2, 1), (0, 0));
        assert_eq!((world.vy_frac[source], world.y_frac[source]), (0, 0));
        assert_eq!(world.get(4, 1), Water);
        // Horizontal impulse changes vx; fixed vertical velocity remains 3.5.
        assert_eq!(world.velocity_at(4, 1), (-1, 3));
        let target = world.idx(4, 1);
        assert_eq!((world.vy_frac[target], world.y_frac[target]), (2, 3));
    }

    fn pressurized_column(material: Material) -> World {
        let mut world = World::new(12, 10);
        for x in 0..12 {
            world.paint(x, 9, Metal);
        }
        world.paint(1, 8, Metal);
        for y in 1..=8 {
            world.paint(2, y, material);
        }
        world
    }

    #[test]
    fn hydrostatic_head_drives_a_horizontal_jet() {
        let mut world = pressurized_column(Water);

        world.apply_liquid_pressure(2, 8, Water);
        assert_eq!(world.velocity_at(2, 8), (3, 0));
        assert!(world.try_velocity_move(2, 8));
        assert_eq!(world.get(5, 8), Water);
        assert_eq!(world.velocity_at(5, 8), (3, 0));
    }

    #[test]
    fn viscosity_limits_pressure_velocity() {
        let mut oil = pressurized_column(Oil);
        oil.apply_liquid_pressure(2, 8, Oil);
        assert_eq!(oil.velocity_at(2, 8), (2, 0));

        let mut lava = pressurized_column(Lava);
        lava.apply_liquid_pressure(2, 8, Lava);
        assert_eq!(lava.velocity_at(2, 8), (1, 0));
    }

    #[test]
    fn unsupported_liquid_does_not_generate_lateral_pressure() {
        let mut world = World::new(5, 6);
        for y in 1..=3 {
            world.paint(2, y, Water);
        }

        world.apply_liquid_pressure(2, 3, Water);
        assert_eq!(world.velocity_at(2, 3), (0, 0));
    }
}
