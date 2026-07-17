use super::*;

impl World {
    /// Detonate with a material-specific blast profile.
    pub(super) fn explode(&mut self, x: usize, y: usize, profile: BlastProfile) {
        let radius = profile.radius;
        let r2 = radius * radius;

        // Inside-out so hard blockers stop the wave before outer cells are hit.
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
        cells.sort_unstable_by_key(|cell| cell.0);

        for &(dist2, dx, dy, tx, ty) in &cells {
            // Epicentre always clears; every other cell needs a free LOS path.
            if dist2 > 0 && !self.blast_has_line_of_sight(x, y, tx, ty) {
                continue;
            }

            // Linear falloff with a floor so cells near the radius edge still hit.
            let strength = blast_strength(dist2, r2);
            let damage = ((profile.damage as i32 * strength) / 1000) as u8;
            let impulse = ((profile.impulse as i32 * strength) / 1000).clamp(0, 16) as i8;
            let heat = ((profile.heat as i32 * strength) / 1000) as i16;

            let i = self.idx(tx, ty);
            let material = self.grid[i];

            // Heat soaks into whatever remains, including resistant walls.
            if heat > 0 {
                self.temp[i] = (self.temp[i] as i32 + heat as i32).clamp(-200, 1_500) as i16;
            }

            if material.is_empty() || material.is_gas() {
                // Soft fill at the core so blasts leave a brief fireball.
                if dist2 == 0 || (strength > 550 && self.roll(tx, ty, 0x92) < 350) {
                    self.put_rand_range(i, Fire, FIRE_LIFE_MIN / 2, FIRE_LIFE_MAX / 2);
                } else if strength > 300 && self.roll(tx, ty, 0x93) < 250 {
                    self.put_rand_range(i, Smoke, SMOKE_LIFE_MIN / 3, SMOKE_LIFE_MAX / 3);
                }
                if impulse > 0 {
                    self.apply_blast_impulse(i, dx, dy, impulse);
                }
                self.activate_next(tx, ty);
                continue;
            }

            // Structural solids accumulate damage and only break past HP.
            if let Some(hp) = material.blast_hp() {
                if self.apply_blast_damage(i, material, damage, hp) {
                    // Debris inherits an outward kick so broken walls collapse away.
                    if impulse > 0 && self.grid[i].is_fluid() {
                        self.apply_blast_impulse(i, dx, dy, impulse);
                    }
                }
                self.activate_next(tx, ty);
                continue;
            }

            // Loose powders/liquids and soft cells: destroy only near the core so the
            // blast throws debris instead of deleting it. Impulse is applied only
            // to survivors so put() cannot wipe the kick.
            if material.is_fluid() {
                if strength > 700 {
                    let roll = self.roll(tx, ty, 0x94);
                    if roll < 400 {
                        self.put_rand_range(i, Fire, FIRE_LIFE_MIN / 2, FIRE_LIFE_MAX / 2);
                        self.activate_next(tx, ty);
                        continue;
                    } else if roll < 700 {
                        self.put_rand_range(i, Smoke, SMOKE_LIFE_MIN / 3, SMOKE_LIFE_MAX / 3);
                        self.activate_next(tx, ty);
                        continue;
                    } else if strength > 850 {
                        self.put(i, Empty, 0);
                        self.activate_next(tx, ty);
                        continue;
                    }
                }
                if impulse > 0 {
                    self.apply_blast_impulse(i, dx, dy, impulse);
                }
                self.activate_next(tx, ty);
                continue;
            }

            // Remaining non-structural solids (TNT/C4/fuse/plant tools, etc.).
            if damage >= 8 {
                let roll = self.roll(tx, ty, 0x95);
                if roll < 500 {
                    self.put_rand_range(i, Fire, FIRE_LIFE_MIN / 2, FIRE_LIFE_MAX / 2);
                } else if roll < 800 {
                    self.put_rand_range(i, Smoke, SMOKE_LIFE_MIN / 3, SMOKE_LIFE_MAX / 3);
                } else {
                    self.put(i, Empty, 0);
                }
            }
            self.activate_next(tx, ty);
        }

        // Atmosphere overpressure / heat from explosion (LOS-aware).
        if self.atmos_enabled {
            self.explode_atmos_effect(x, y, profile);
        }
    }

    /// Supercover line of sight: blast energy is blocked by hard solids.
    /// The destination cell is excluded so a wall can still take the hit that
    /// stops further propagation beyond it.
    pub(super) fn blast_has_line_of_sight(
        &self,
        x0: usize,
        y0: usize,
        x1: usize,
        y1: usize,
    ) -> bool {
        let mut x = x0 as i32;
        let mut y = y0 as i32;
        let x1 = x1 as i32;
        let y1 = y1 as i32;
        let dx = (x1 - x).abs();
        let dy = (y1 - y).abs();
        let sx = if x < x1 { 1 } else { -1 };
        let sy = if y < y1 { 1 } else { -1 };
        let mut err = dx - dy;

        loop {
            if x == x1 && y == y1 {
                return true;
            }
            // Skip the origin; check every crossed cell before the target.
            if !(x == x0 as i32 && y == y0 as i32) {
                let i = self.idx(x as usize, y as usize);
                if self.blast_blocks_wave(self.grid[i]) {
                    return false;
                }
            }

            let e2 = err * 2;
            let step_x = e2 > -dy;
            let step_y = e2 < dx;

            // A diagonal ray crosses both cells touching the corner. Treat either
            // as an occluder instead of allowing blast energy through a crack.
            if step_x && step_y {
                for (cx, cy) in [(x + sx, y), (x, y + sy)] {
                    if (cx != x1 || cy != y1)
                        && self.blast_blocks_wave(self.grid[self.idx(cx as usize, cy as usize)])
                    {
                        return false;
                    }
                }
            }

            if step_x {
                err -= dy;
                x += sx;
            }
            if step_y {
                err += dx;
                y += sy;
            }
            if !step_x && !step_y {
                return false;
            }
        }
    }

    /// Hard solids stop blast propagation; broken debris and fluids do not.
    fn blast_blocks_wave(&self, material: Material) -> bool {
        material.blast_hp().is_some()
    }

    /// Add accumulated blast damage. Breaks the cell into its debris product
    /// once damage meets the material HP. Returns true when the cell broke.
    pub(super) fn apply_blast_damage(
        &mut self,
        i: usize,
        material: Material,
        damage: u8,
        hp: u8,
    ) -> bool {
        if damage == 0 {
            return false;
        }
        // life is reused as soak/ignition counters for many materials; for
        // structural solids it now also tracks accumulated blast damage.
        let next = self.life[i].saturating_add(damage as u16);
        if next < hp as u16 {
            self.life[i] = next;
            self.activate_idx(i);
            return false;
        }

        let product = material.blast_break_product().unwrap_or(Empty);
        let life = self.rand_life(product);
        self.put(i, product, life);
        true
    }

    /// Apply an outward fixed-point impulse along `(dx, dy)` without relocating
    /// the cell. Velocity-driven movement throws the debris on later ticks.
    pub(super) fn apply_blast_impulse(&mut self, i: usize, dx: i32, dy: i32, impulse: i8) {
        if impulse == 0 || (dx == 0 && dy == 0) {
            return;
        }
        // Normalize direction to unit steps so diagonals do not get double force.
        let sx = dx.signum();
        let sy = dy.signum();
        let scale = if sx != 0 && sy != 0 {
            // Approximate 1/sqrt(2) so diagonal kicks match cardinal strength.
            (impulse as i32 * 3) / 4
        } else {
            impulse as i32
        };
        let ix = (sx * scale).clamp(-16, 16) as i8;
        let iy = (sy * scale).clamp(-16, 16) as i8;
        if ix != 0 {
            self.apply_horizontal_impulse(i, ix, self.grid[i]);
        }
        if iy != 0 {
            self.apply_vertical_impulse(i, iy, self.grid[i]);
        }
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
        let life = self.rand_life(product);
        self.put(i, product, life);
        true
    }

    pub(super) fn step_tnt(&mut self, x: usize, y: usize) {
        if self.is_heated(x, y) {
            self.explode(x, y, TNT_BLAST);
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
        self.put_rand_range(i, Fire, FIRE_LIFE_MIN, FIRE_LIFE_MAX);
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
            self.explode(x, y, C4_BLAST);
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

        // Gunpowder blast respects structural HP / LOS via explode().
        if m == Gunpowder && self.is_heated(x, y) {
            self.explode(x, y, GUNPOWDER_BLAST);
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

        // Single-cell diagonal resolution after a blocked fall.
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
    /// column is supported and has somewhere to discharge. Shallow surface cells
    /// still get a unit impulse so pools level without multi-cell teleports.
    fn apply_liquid_pressure(&mut self, x: usize, y: usize, m: Material) {
        let cap = pressure_speed_of(m);
        if cap == 0 {
            return;
        }

        let supported = self
            .adj(x, y, 0, 1)
            .is_none_or(|(bx, by)| !m.can_sink_into(self.get(bx, by)));
        if !supported {
            // Free-falling liquid should drop, not keep sliding past an outlet.
            let i = self.idx(x, y);
            if self.vx[i] != 0 || self.vx_frac[i] != 0 || self.x_frac[i] != 0 {
                self.vx[i] = 0;
                self.vx_frac[i] = 0;
                self.x_frac[i] = 0;
                self.activate_idx(i);
            }
            return;
        }

        // Sticky gels cling to solid support instead of discharging sideways.
        if m.sticky() {
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

        let left_open = x > 0 && self.get(x - 1, y) == Empty;
        let right_open = x + 1 < self.width && self.get(x + 1, y) == Empty;
        let dir = match (left_open, right_open) {
            (true, false) => -1,
            (false, true) => 1,
            (true, true) => {
                // Prefer the side that drops soonest so free surfaces drain
                // into open basins instead of jittering in place.
                let left_drop = self.surface_drop_score(x, y, -1);
                let right_drop = self.surface_drop_score(x, y, 1);
                match left_drop.cmp(&right_drop) {
                    std::cmp::Ordering::Greater => -1,
                    std::cmp::Ordering::Less => 1,
                    std::cmp::Ordering::Equal => self.dirs(x, y, 0x22).0,
                }
            }
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

    /// Distance-weighted preference for an open downward step along a free surface.
    fn surface_drop_score(&self, x: usize, y: usize, dir: i32) -> usize {
        let mut cx = x;
        for distance in 1..=4 {
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
                return 5 - distance;
            }
            cx = nx;
        }
        0
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

        // Napalm clings to solids: damp lateral sliding and skip hydrostatic
        // discharge so shallow unit impulses cannot skate it off a ledge.
        // Rare drips use a single-cell lateral step instead of velocity jets.
        let clinging = m.sticky()
            && self.adj(x, y, 0, 1).is_some_and(|(bx, by)| {
                let below = self.grid[self.idx(bx, by)];
                !below.is_empty() && !below.is_fluid()
            });
        if clinging {
            if self.vx[i] != 0 || self.vx_frac[i] != 0 || self.x_frac[i] != 0 {
                self.vx[i] = 0;
                self.vx_frac[i] = 0;
                self.x_frac[i] = 0;
                self.activate_idx(i);
            }
            if self.chance(x, y, 0x55, 80) {
                let (d1, d2) = self.dirs(x, y, 0x20);
                for d in [d1, d2] {
                    if self.try_step(x, y, d, 0, |t| t == Empty) {
                        return;
                    }
                }
            }
            return;
        }

        // Apply vertical force and hydrostatic pressure after lifecycle/reaction
        // checks. Supported columns and free surfaces discharge sideways with
        // speed proportional to head; falling liquid remains gravity-driven.
        self.apply_vertical_force(i);
        self.apply_liquid_pressure(x, y, m);
        let has_vertical_step = self.has_vertical_step(i);

        // Velocity-driven movement only: lateral leveling is carried by
        // hydrostatic impulses above, not by multi-cell teleports.
        if self.try_velocity_move(x, y) {
            return;
        }
        if !has_vertical_step {
            return;
        }

        let sink = |other| m.can_sink_into(other);
        let rise = |t: Material| t.is_fluid() && m.density() < t.density();

        // Single-cell diagonal resolution after a blocked fall.
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
        let _ = self.try_step(x, y, 0, -1, rise);
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

        // Single-cell residual dispersion under ceilings when buoyancy is blocked.
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
            // Atmosphere-off mode: smother when no free neighbour remains.
            let mut has_air = false;
            for (nx, ny) in self.n8(x, y).into_iter().flatten() {
                let ni = self.idx(nx, ny);
                let other = self.grid[ni];
                has_air |= matches!(other, Empty | Smoke | Steam | Fire) || other.flammable();
            }
            if !has_air {
                self.put_rand_range(i, Smoke, SMOKE_LIFE_MIN / 2, SMOKE_LIFE_MAX / 2);
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
            self.put_rand_range(ni, Steam, STEAM_LIFE_MIN, STEAM_LIFE_MAX);
            if !oily {
                extinguished = true;
            }
            // greasy fires: water steams off without killing the flame
        }

        if extinguished {
            // The adjacent water already became steam. Turning the flame into
            // steam too would create an extra water cell when both condense.
            self.put_rand_range(i, Smoke, SMOKE_LIFE_MIN / 2, SMOKE_LIFE_MAX / 2);
            return;
        }

        // Oxygen-starved flames make more visible smoke as combustion dirties.
        let smoke_chance = if self.atmos_enabled {
            match self.oxygen_percent(i) {
                percent if percent < CRITICAL_OXYGEN_PERCENT => 160,
                percent if percent < LOW_OXYGEN_PERCENT => 80,
                _ => 20,
            }
        } else {
            20
        };
        if self.chance(x, y, 0x43, smoke_chance)
            && let Some((ux, uy)) = self.adj(x, y, 0, -1)
        {
            let ui = self.idx(ux, uy);
            if self.grid[ui] == Empty {
                self.put_rand_range(ui, Smoke, SMOKE_LIFE_MIN, SMOKE_LIFE_MAX);
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
                    self.put_rand_range(ni, Steam, STEAM_LIFE_MIN, STEAM_LIFE_MAX);
                    quenched = true;
                }
                Empty | Smoke if ny < y => {
                    // Flames can lick upward through smoke; smoke wisps are less common.
                    let r = self.roll(nx, ny, 0x53);
                    if r < 80 {
                        self.put_rand_range(ni, Fire, FIRE_LIFE_MIN, FIRE_LIFE_MAX);
                    } else if r < 100 {
                        self.put_rand_range(ni, Smoke, SMOKE_LIFE_MIN, SMOKE_LIFE_MAX);
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
    fn extinguishing_fire_does_not_create_extra_water_mass() {
        let mut world = World::new(4, 3);
        world.paint(1, 1, Water);
        world.paint(2, 1, Fire);

        world.step_fire(2, 1);

        let water_mass = world
            .grid
            .iter()
            .filter(|&&material| matches!(material, Water | Steam))
            .count();
        assert_eq!(water_mass, 1);
        assert_eq!(world.get(1, 1), Steam);
        assert_eq!(world.get(2, 1), Smoke);
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

    #[test]
    fn sticky_liquid_clings_instead_of_pressurizing_off_support() {
        let mut world = World::new(9, 9);
        for x in 0..9 {
            world.paint(x, 8, Metal);
        }
        for x in 3..6 {
            world.paint(x, 7, Metal);
            world.paint(x, 6, Napalm);
        }

        // Direct pressure must not skate clinging napalm sideways.
        world.apply_liquid_pressure(3, 6, Napalm);
        assert_eq!(world.velocity_at(3, 6), (0, 0));

        for _ in 0..40 {
            world.step();
        }
        let still_on_ledge = (3..6).filter(|&x| world.get(x, 6) == Napalm).count();
        assert!(
            still_on_ledge > 0,
            "napalm should remain on the solid ledge"
        );
    }

    #[test]
    fn blast_line_of_sight_excludes_target_but_blocks_cells_behind_it() {
        let mut world = World::new(5, 1);
        world.paint(2, 0, Metal);

        assert!(world.blast_has_line_of_sight(0, 0, 2, 0));
        assert!(!world.blast_has_line_of_sight(0, 0, 4, 0));
    }

    #[test]
    fn blast_line_of_sight_cannot_pass_through_diagonal_corner() {
        let mut world = World::new(3, 3);
        world.paint(1, 0, Metal);

        assert!(!world.blast_has_line_of_sight(0, 0, 2, 2));
    }

    /// Two worlds initialised with the same seed must produce identical state
    /// after the same number of steps, confirming deterministic reproducibility.
    #[test]
    fn same_seed_produces_identical_simulation() {
        const STEPS: usize = 100;
        let mut a = World::with_seed(32, 24, 12345);
        let mut b = World::with_seed(32, 24, 12345);

        for _ in 0..STEPS {
            a.step();
            b.step();
        }

        // Compare every cell.
        for y in 0..a.height {
            for x in 0..a.width {
                let ia = a.idx(x, y);
                let ib = b.idx(x, y);
                assert_eq!(a.grid[ia], b.grid[ib], "cell ({x},{y}) material mismatch");
                assert_eq!(a.life[ia], b.life[ib], "cell ({x},{y}) life mismatch");
                assert_eq!(a.seed[ia], b.seed[ib], "cell ({x},{y}) seed mismatch");
                assert_eq!(a.temp[ia], b.temp[ib], "cell ({x},{y}) temp mismatch");
            }
        }
    }
}
