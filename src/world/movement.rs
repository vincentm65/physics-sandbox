use super::*;

impl World {
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
    /// component on the moving cell. Returns true if the cell changed position.
    pub(super) fn try_velocity_move(&mut self, x: usize, y: usize) -> bool {
        let i = self.idx(x, y);
        let vx = self.vx[i];
        let vy = self.vy[i];
        let vx_frac = self.vx_frac[i];
        let x_frac = self.x_frac[i];
        let vy_frac = self.vy_frac[i];
        let y_frac = self.y_frac[i];

        // Compute effective integer displacements from fixed-point state.
        // Horizontal and vertical axes both use quarter-cell fractions so small
        // blast/pressure impulses eventually produce whole-cell movement.
        let x_sum = (x_frac as i16) + (vx_frac as i16);
        let x_carry = x_sum / (VELOCITY_SCALE as i16);
        let new_x_frac = x_sum % (VELOCITY_SCALE as i16);
        let dx = (vx as i16 + x_carry) as i8;

        let y_sum = (y_frac as i16) + (vy_frac as i16);
        let y_carry = y_sum / (VELOCITY_SCALE as i16);
        let new_y_frac = y_sum % (VELOCITY_SCALE as i16);
        let dy = (vy as i16 + y_carry) as i8;

        // Write back updated sub-cell positions before any movement.
        self.x_frac[i] = new_x_frac as i8;
        self.y_frac[i] = new_y_frac as i8;

        if dx == 0 && dy == 0 {
            // No integer movement this tick, but if sub-cell motion exists,
            // keep the cell active so acceleration (gravity/impulse) continues.
            if vx != 0
                || vy != 0
                || vx_frac != 0
                || vy_frac != 0
                || new_x_frac != 0
                || new_y_frac != 0
            {
                self.activate_idx(i);
            }
            return false;
        }

        let material = self.grid[i];
        // Only non-structural movable materials use velocity-driven movement.
        if !material.is_fluid() {
            return false;
        }

        let sx = dx.signum() as i32;
        let sy = dy.signum() as i32;
        let adx = dx.unsigned_abs() as i32;
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
                    self.vx_frac[ci] = 0;
                    self.x_frac[ci] = 0;
                    blocked = true;
                } else {
                    let ni = self.idx(nx as usize, cy as usize);
                    if self.grid[ni].is_empty() {
                        self.swap(ci, ni);
                        cx = nx;
                        moved = true;
                    } else {
                        self.vx[ci] = 0;
                        self.vx_frac[ci] = 0;
                        self.x_frac[ci] = 0;
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

    /// Atmosphere-aware lift for visible gas materials. Buoyancy vanishes in a
    /// near-vacuum, strengthens in dense air, and increases with gas temperature.
    fn visible_gas_buoyancy(&self, i: usize) -> i8 {
        if !self.atmos_enabled {
            return -GRAVITY_PER_TICK;
        }
        let air = self.air_mass[i].max(0) as i32;
        if air < AMBIENT_AIR_MASS as i32 / 8 {
            return 0;
        }
        let density_lift = (air + AMBIENT_AIR_MASS as i32 - 1) / AMBIENT_AIR_MASS as i32;
        let thermal_lift = (self.temp[i] as i32 - AMBIENT_TEMP as i32).max(0) / 300;
        -(density_lift + thermal_lift).clamp(1, 4) as i8
    }

    /// Apply the appropriate vertical force to the cell at `i` based on its
    /// material classification. Gravity accelerates powders, liquids, embers,
    /// and sparks downward; atmosphere-aware buoyancy lifts gases and fire.
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
            Some(self.visible_gas_buoyancy(i))
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
}
