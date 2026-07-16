//! Atmosphere simulation: deterministic transport of air mass and gas species,
//! equal-pressure mixing, combustion, open-edge venting, and pressure-gradient
//! impulses. No per-tick heap allocation after initialization.

use crate::material::{AMBIENT_TEMP, Material};

use super::*;

// ── Atmosphere cell topology ──────────────────────────────────────────────

/// Cells that allow gas flow (passable). All solids and liquids block.
/// This mirrors `World::is_gas_permeable` but is a pure function for use
/// here without borrowing the world.
pub(super) fn cell_is_passable(m: Material) -> bool {
    m.is_empty() || m.is_gas()
}

// ── Transport helpers ─────────────────────────────────────────────────────

/// Clamp a value to [0, MAX_AIR_MASS].
fn clamp_mass(v: i16) -> i16 {
    v.clamp(0, MAX_AIR_MASS)
}

/// Total explicitly tracked species mass in a cell. The remainder is inert air.
fn species_total(o2: i16, exhaust: i16, fuel_vapor: i16) -> i16 {
    o2.saturating_add(exhaust).saturating_add(fuel_vapor)
}

/// Absolute fixed-point temperature used by the ideal-gas calculations.
fn temp_k(temp: i16) -> i32 {
    (temp as i32).max(-200) + 273
}

// ── Public methods on World ───────────────────────────────────────────────

impl World {
    /// Ambient‑initialize all passable cells; zero atmosphere in blocked cells.
    pub fn ambient_init_atmosphere(&mut self) {
        for i in 0..self.grid.len() {
            if cell_is_passable(self.grid[i]) {
                self.air_mass[i] = AMBIENT_AIR_MASS;
                self.o2[i] = AMBIENT_O2;
            } else {
                self.air_mass[i] = 0;
                self.o2[i] = 0;
            }
            self.exhaust[i] = 0;
            self.fuel_vapor[i] = 0;
        }
    }

    /// Set atmosphere simulation enabled state.
    pub fn set_air_enabled(&mut self, enabled: bool) {
        self.atmos_enabled = enabled;
    }

    /// Single step of atmosphere simulation.  Called from `World::step`
    /// only when `self.atmos_enabled` is true.
    pub(super) fn step_atmosphere(&mut self) {
        // 1. Combustion: fire consumes O₂, hot oil/napalm emits fuel vapor,
        //    fuel‑vapor ignition, overpressure.
        self.step_combustion_atmos();

        // 2. Transport: diffuse air mass and proportional species through
        //    passable paths, vent at open edges.
        self.step_transport_atmos();
    }

    // ── Combustion integration ─────────────────────────────────────────

    /// Handle all atmosphere‑related chemistry each tick.
    fn step_combustion_atmos(&mut self) {
        // We iterate over active chunks and look for fire/fuel cells.
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
                        match self.grid[i] {
                            Fire => self.atmos_fire_burn(x, y, i),
                            Oil | Napalm => self.atmos_oil_vapor(x, y, i),
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    /// Local oxygen concentration as a percentage of air mass.
    pub(super) fn oxygen_percent(&self, i: usize) -> i32 {
        let air = self.air_mass[i] as i32;
        if air <= 0 {
            0
        } else {
            self.o2[i] as i32 * 100 / air
        }
    }

    /// Heat emitted by a flame at its current oxygen stage.
    pub(super) fn fire_heat(&self, i: usize) -> i16 {
        if !self.atmos_enabled {
            return 900;
        }
        match self.oxygen_percent(i) {
            percent if percent < CRITICAL_OXYGEN_PERCENT => 250,
            percent if percent < LOW_OXYGEN_PERCENT => 500,
            _ => 900,
        }
    }

    /// Fire consumes the effective 2.5D oxygen reserve gradually. Flames beside
    /// combustible fuel draw the full budget; free flames draw half as much.
    /// Low oxygen weakens combustion, while critical starvation shortens flame
    /// life over several dozen ticks instead of extinguishing it immediately.
    fn atmos_fire_burn(&mut self, x: usize, y: usize, i: usize) {
        if self.oxygen_percent(i) < CRITICAL_OXYGEN_PERCENT {
            self.draw_o2_from_neighbors(x, y, 1);
        }

        let oxygen_percent = self.oxygen_percent(i);
        if oxygen_percent < CRITICAL_OXYGEN_PERCENT {
            self.life[i] = self.life[i].saturating_sub(CRITICAL_FIRE_LIFE_LOSS);
            if self.life[i] == 0 {
                let min_life = SMOKE_LIFE_MIN / 2;
                let life_span = SMOKE_LIFE_MAX / 2 - min_life;
                let life = min_life + (self.roll(x, y, 0xA7) as u16 * life_span / 999);
                self.put(i, Smoke, life);
            }
            return;
        }

        let beside_fuel = self
            .n8(x, y)
            .into_iter()
            .flatten()
            .any(|(nx, ny)| self.grid[self.idx(nx, ny)].flammable());
        let interval = if beside_fuel {
            FIRE_OXYGEN_INTERVAL
        } else {
            FREE_FLAME_OXYGEN_INTERVAL
        };
        let interval = if oxygen_percent < LOW_OXYGEN_PERCENT {
            interval * 2
        } else {
            interval
        };
        if self.tick.wrapping_add(i as u64).is_multiple_of(interval) {
            let consume = 1.min(self.o2[i]);
            self.o2[i] = clamp_mass(self.o2[i].saturating_sub(consume));
            self.exhaust[i] = clamp_mass(self.exhaust[i].saturating_add(consume));
        }

        // Fuel vapor remains energetic, but its effective depth limits the burn
        // front to one fixed-point unit per tick.
        let vapor = self.fuel_vapor[i];
        if vapor > 0 {
            let burn = (MAX_TRANSPORT / COMBUSTION_DEPTH as i16)
                .max(1)
                .min(vapor)
                .min(self.o2[i]);
            if burn > 0 {
                self.fuel_vapor[i] = clamp_mass(self.fuel_vapor[i].saturating_sub(burn));
                self.o2[i] = clamp_mass(self.o2[i].saturating_sub(burn));
                self.exhaust[i] =
                    clamp_mass(self.exhaust[i].saturating_add(burn.saturating_mul(2)));
                self.temp[i] = (self.temp[i] as i32 + burn as i32 * 8).clamp(-200, 1_500) as i16;
                let overpressure = (burn as i16).min(MAX_AIR_MASS.saturating_sub(self.air_mass[i]));
                if overpressure > 0 {
                    self.air_mass[i] = clamp_mass(self.air_mass[i].saturating_add(overpressure));
                }
                self.activate_next(x, y);
            }
        }
    }

    /// Draw O₂ from adjacent passable cells up to `max`.  Returns amount drawn.
    fn draw_o2_from_neighbors(&mut self, x: usize, y: usize, max: i16) -> i16 {
        let i = self.idx(x, y);
        let mut drawn = 0i16;
        let mut remaining = max;
        let dirs = [(0i32, -1i32), (0, 1), (-1, 0), (1, 0)];
        for (dx, dy) in dirs {
            if remaining <= 0 {
                break;
            }
            if let Some((nx, ny)) = self.adj(x, y, dx, dy) {
                let ni = self.idx(nx, ny);
                if !cell_is_passable(self.grid[ni]) {
                    continue;
                }
                let take = remaining.min(self.o2[ni]);
                if take > 0 {
                    self.o2[ni] = clamp_mass(self.o2[ni].saturating_sub(take));
                    self.o2[i] = clamp_mass(self.o2[i].saturating_add(take));
                    drawn += take;
                    remaining -= take;
                    self.activate_next(nx, ny);
                }
            }
        }
        drawn
    }

    /// Oil / Napalm emits fuel vapor to adjacent passable cells when hot.
    fn atmos_oil_vapor(&mut self, x: usize, y: usize, _i: usize) {
        // Only emit when sufficiently hot.
        let effective = self.effective_temp(x, y);
        if effective < 150 {
            return;
        }
        // Emit a small amount of fuel vapor into each passable neighbour.
        let dirs = [(0i32, -1i32), (0, 1), (-1, 0), (1, 0)];
        for (dx, dy) in dirs {
            if let Some((nx, ny)) = self.adj(x, y, dx, dy) {
                let ni = self.idx(nx, ny);
                if !cell_is_passable(self.grid[ni]) {
                    continue;
                }
                let available = self.air_mass[ni]
                    .saturating_sub(species_total(
                        self.o2[ni],
                        self.exhaust[ni],
                        self.fuel_vapor[ni],
                    ))
                    .max(0);
                let added = 1i16.min(available);
                if added > 0 {
                    self.fuel_vapor[ni] = clamp_mass(self.fuel_vapor[ni].saturating_add(added));
                    self.activate_next(nx, ny);
                }
            }
        }
    }

    // ── Transport ──────────────────────────────────────────────────────

    /// Diffuse air mass and proportional gas species through 4‑directional
    /// paths.  Open world edges vent to ambient.  Equal‑pressure species
    /// mixing at each pair.
    fn step_transport_atmos(&mut self) {
        // We need a temporary cell that can hold one extra species array
        // (or we just look at active chunks and diffuse between adjacent
        // passable cells in two passes).
        //
        // Strategy: for each active chunk, for each cell, for each of the 4
        // cardinal directions with smaller index (to avoid double‑counting),
        // transport air mass and species proportionally.
        //
        // We always work with in‑place modifications using the principle that
        // transport is symmetric: the same amount leaves one cell and arrives
        // at the other.

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
                        if !cell_is_passable(self.grid[i]) {
                            continue;
                        }
                        // Right neighbour.
                        if x + 1 < self.width {
                            let ri = self.idx(x + 1, y);
                            if cell_is_passable(self.grid[ri]) && self.transport_pair(i, ri) {
                                self.activate_next(x, y);
                                self.activate_next(x + 1, y);
                            }
                        }
                        // Down neighbour.
                        if y + 1 < self.height {
                            let di = self.idx(x, y + 1);
                            if cell_is_passable(self.grid[di])
                                && self.transport_vertical_pair(i, di)
                            {
                                self.activate_next(x, y);
                                self.activate_next(x, y + 1);
                            }
                        }
                    }
                }
            }
        }

        // Edge venting: world edges open to ambient atmosphere.
        // Top edge.
        for x in 0..self.width {
            let i = self.idx(x, 0);
            if cell_is_passable(self.grid[i]) {
                self.vent_to_ambient(i);
            }
        }
        // Bottom edge.
        for x in 0..self.width {
            let i = self.idx(x, self.height - 1);
            if cell_is_passable(self.grid[i]) {
                self.vent_to_ambient(i);
            }
        }
        // Left edge.
        for y in 0..self.height {
            let i = self.idx(0, y);
            if cell_is_passable(self.grid[i]) {
                self.vent_to_ambient(i);
            }
        }
        // Right edge.
        for y in 0..self.height {
            let i = self.idx(self.width - 1, y);
            if cell_is_passable(self.grid[i]) {
                self.vent_to_ambient(i);
            }
        }
    }

    /// Vertical transport includes molecular-weight stratification. Compare the
    /// final state so a stable stratified pair can become inactive.
    fn transport_vertical_pair(&mut self, upper: usize, lower: usize) -> bool {
        let before = (
            self.air_mass[upper],
            self.air_mass[lower],
            self.o2[upper],
            self.o2[lower],
            self.exhaust[upper],
            self.exhaust[lower],
            self.fuel_vapor[upper],
            self.fuel_vapor[lower],
            self.temp[upper],
            self.temp[lower],
        );
        self.transport_pair(upper, lower);
        self.stratify_species(upper, lower);
        before
            != (
                self.air_mass[upper],
                self.air_mass[lower],
                self.o2[upper],
                self.o2[lower],
                self.exhaust[upper],
                self.exhaust[lower],
                self.fuel_vapor[upper],
                self.fuel_vapor[lower],
                self.temp[upper],
                self.temp[lower],
            )
    }

    /// Transport air between adjacent passable cells toward equal pressure.
    /// The bounded packet carries heat; gas species then mix conservatively.
    fn transport_pair(&mut self, a: usize, b: usize) -> bool {
        let before = (
            self.air_mass[a],
            self.air_mass[b],
            self.o2[a],
            self.o2[b],
            self.exhaust[a],
            self.exhaust[b],
            self.fuel_vapor[a],
            self.fuel_vapor[b],
            self.temp[a],
            self.temp[b],
        );
        let pressure_a = self.air_mass[a] as i32 * temp_k(self.temp[a]);
        let pressure_b = self.air_mass[b] as i32 * temp_k(self.temp[b]);
        let (src, dst) = if pressure_a > pressure_b {
            (a, b)
        } else {
            (b, a)
        };
        let src_mass = self.air_mass[src].max(0) as i32;
        let dst_mass = self.air_mass[dst].max(0) as i32;
        let src_temp_k = temp_k(self.temp[src]);
        let dst_temp_k = temp_k(self.temp[dst]);
        let pressure_diff = (src_mass * src_temp_k - dst_mass * dst_temp_k).max(0);
        let equilibrium_packet = pressure_diff / (src_temp_k + dst_temp_k).max(1);
        let transport = equilibrium_packet
            .min(MAX_TRANSPORT as i32)
            .min(src_mass)
            .min(MAX_AIR_MASS as i32 - dst_mass) as i16;

        if transport == 0
            && self.o2[a] == self.o2[b]
            && self.exhaust[a] == self.exhaust[b]
            && self.fuel_vapor[a] == self.fuel_vapor[b]
        {
            return false;
        }

        if transport > 0 {
            self.air_mass[src] = clamp_mass(self.air_mass[src] - transport);
            self.air_mass[dst] = clamp_mass(self.air_mass[dst] + transport);

            // A homogeneous source keeps its temperature. The destination mixes
            // the incoming packet's thermal energy with its existing air.
            let mixed_mass = dst_mass + transport as i32;
            if mixed_mass > 0 {
                let mixed_k = (dst_mass * dst_temp_k + transport as i32 * src_temp_k) / mixed_mass;
                self.temp[dst] = (mixed_k - 273).clamp(-200, 1_500) as i16;
            }
        }

        self.equalize_species(a, b);
        before
            != (
                self.air_mass[a],
                self.air_mass[b],
                self.o2[a],
                self.o2[b],
                self.exhaust[a],
                self.exhaust[b],
                self.fuel_vapor[a],
                self.fuel_vapor[b],
                self.temp[a],
                self.temp[b],
            )
    }

    /// Conservatively equalize each species concentration between two cells.
    fn equalize_species(&mut self, a: usize, b: usize) {
        let ma = self.air_mass[a].max(0) as i32;
        let mb = self.air_mass[b].max(0) as i32;
        let total_mass = ma + mb;
        if total_mass <= 0 {
            return;
        }

        let totals = [
            self.o2[a].max(0) as i32 + self.o2[b].max(0) as i32,
            self.exhaust[a].max(0) as i32 + self.exhaust[b].max(0) as i32,
            self.fuel_vapor[a].max(0) as i32 + self.fuel_vapor[b].max(0) as i32,
        ];
        let mut a_shares = totals.map(|total| total * ma / total_mass);
        let mut excess_b = (totals.iter().sum::<i32>() - a_shares.iter().sum::<i32>() - mb).max(0);
        for (share, total) in a_shares.iter_mut().zip(totals) {
            let moved = excess_b.min(total - *share);
            *share += moved;
            excess_b -= moved;
        }

        self.o2[a] = a_shares[0] as i16;
        self.o2[b] = (totals[0] - a_shares[0]) as i16;
        self.exhaust[a] = a_shares[1] as i16;
        self.exhaust[b] = (totals[1] - a_shares[1]) as i16;
        self.fuel_vapor[a] = a_shares[2] as i16;
        self.fuel_vapor[b] = (totals[2] - a_shares[2]) as i16;
        self.clamp_species_to_mass(a);
        self.clamp_species_to_mass(b);
    }

    /// Apply a one-unit gravitational drift to tracked gases after ordinary
    /// mixing. Molecular weight is compared at local absolute temperature, so
    /// cool oxygen/combustion products settle while sufficiently hot gas rises.
    fn stratify_species(&mut self, upper: usize, lower: usize) -> bool {
        let mut changed = false;
        changed |= self.stratify_component(upper, lower, 0, 32);
        changed |= self.stratify_component(upper, lower, 1, 44);
        changed |= self.stratify_component(upper, lower, 2, 44);
        changed
    }

    fn stratify_component(
        &mut self,
        upper: usize,
        lower: usize,
        component: u8,
        molecular_weight: i32,
    ) -> bool {
        let values = match component {
            0 => (self.o2[upper], self.o2[lower]),
            1 => (self.exhaust[upper], self.exhaust[lower]),
            _ => (self.fuel_vapor[upper], self.fuel_vapor[lower]),
        };
        let upper_inert = self.air_mass[upper]
            .saturating_sub(species_total(
                self.o2[upper],
                self.exhaust[upper],
                self.fuel_vapor[upper],
            ))
            .max(0);
        let lower_inert = self.air_mass[lower]
            .saturating_sub(species_total(
                self.o2[lower],
                self.exhaust[lower],
                self.fuel_vapor[lower],
            ))
            .max(0);
        let ambient_k = temp_k(AMBIENT_TEMP);
        let upper_weight = molecular_weight * ambient_k / temp_k(self.temp[upper]);
        let lower_weight = molecular_weight * ambient_k / temp_k(self.temp[lower]);

        let (new_upper, new_lower) = if lower_weight < 28
            && lower_weight < upper_weight
            && values.1 > 0
            && upper_inert > 0
        {
            (values.0 + 1, values.1 - 1)
        } else if upper_weight > 28
            && upper_weight >= lower_weight
            && values.0 > 0
            && lower_inert > 0
        {
            (values.0 - 1, values.1 + 1)
        } else {
            return false;
        };

        match component {
            0 => {
                self.o2[upper] = new_upper;
                self.o2[lower] = new_lower;
            }
            1 => {
                self.exhaust[upper] = new_upper;
                self.exhaust[lower] = new_lower;
            }
            _ => {
                self.fuel_vapor[upper] = new_upper;
                self.fuel_vapor[lower] = new_lower;
            }
        }
        true
    }

    /// Preserve the tracked-mixture invariant after chemistry or external input.
    fn clamp_species_to_mass(&mut self, i: usize) {
        self.o2[i] = clamp_mass(self.o2[i]);
        self.exhaust[i] = clamp_mass(self.exhaust[i]);
        self.fuel_vapor[i] = clamp_mass(self.fuel_vapor[i]);
        let mut excess = species_total(self.o2[i], self.exhaust[i], self.fuel_vapor[i])
            .saturating_sub(self.air_mass[i].max(0))
            .max(0);
        let trim_fuel = excess.min(self.fuel_vapor[i]);
        self.fuel_vapor[i] -= trim_fuel;
        excess -= trim_fuel;
        let trim_exhaust = excess.min(self.exhaust[i]);
        self.exhaust[i] -= trim_exhaust;
        excess -= trim_exhaust;
        self.o2[i] = self.o2[i].saturating_sub(excess);
    }

    /// Exchange an edge cell with the infinite ambient reservoir. Pressure flow
    /// and composition mixing are independent, so exhaust clears at 1 atm too.
    fn vent_to_ambient(&mut self, i: usize) {
        let before = (
            self.air_mass[i],
            self.o2[i],
            self.exhaust[i],
            self.fuel_vapor[i],
            self.temp[i],
        );
        let mass = self.air_mass[i].max(0);
        let local_k = temp_k(self.temp[i]);
        let ambient_k = temp_k(AMBIENT_TEMP);
        let target_mass = ((AMBIENT_AIR_MASS as i32 * ambient_k + local_k / 2) / local_k)
            .clamp(0, MAX_AIR_MASS as i32) as i16;

        if mass < target_mass {
            let pull = MAX_TRANSPORT.min(target_mass - mass);
            let new_mass = mass + pull;
            let mixed_k =
                (mass as i32 * local_k + pull as i32 * ambient_k) / new_mass.max(1) as i32;
            self.air_mass[i] = new_mass;
            self.temp[i] = (mixed_k - 273).clamp(-200, 1_500) as i16;
            let ambient_share =
                |air_mass: i16| air_mass as i32 * AMBIENT_O2 as i32 / AMBIENT_AIR_MASS as i32;
            self.o2[i] =
                clamp_mass(self.o2[i] + (ambient_share(new_mass) - ambient_share(mass)) as i16);
        } else if mass > target_mass {
            let vent = MAX_TRANSPORT.min(mass - target_mass);
            let remaining = mass - vent;
            let retain = |species: i16| species as i32 * remaining as i32 / mass.max(1) as i32;
            self.air_mass[i] = remaining;
            self.o2[i] = retain(self.o2[i]) as i16;
            self.exhaust[i] = retain(self.exhaust[i]) as i16;
            self.fuel_vapor[i] = retain(self.fuel_vapor[i]) as i16;
        }

        // Molecular exchange occurs without net mass flow. Move each tracked
        // species one fixed-point unit toward ambient composition per tick.
        let ambient_o2 = self.air_mass[i] as i32 * AMBIENT_O2 as i32 / AMBIENT_AIR_MASS as i32;
        self.o2[i] += (ambient_o2 as i16 - self.o2[i]).signum();
        self.exhaust[i] = self.exhaust[i].saturating_sub(1);
        self.fuel_vapor[i] = self.fuel_vapor[i].saturating_sub(1);
        self.clamp_species_to_mass(i);

        if before
            != (
                self.air_mass[i],
                self.o2[i],
                self.exhaust[i],
                self.fuel_vapor[i],
                self.temp[i],
            )
        {
            self.activate_idx(i);
        }
    }

    /// Compute pressure from air mass and local temperature.
    /// Uses ideal‑gas approximation: P ∝ air_mass * (temp + 273) / AMBIENT_AIR_MASS
    pub(super) fn cell_pressure(&self, i: usize) -> i32 {
        let mass = self.air_mass[i] as i32;
        if mass <= 0 {
            return 0;
        }
        let temp_k = (self.temp[i] as i32).max(-200) + 273;
        let ambient_k = (AMBIENT_TEMP as i32) + 273;
        // Normalised pressure: 256 = 1 atm.
        mass * temp_k * 256 / (AMBIENT_AIR_MASS as i32 * ambient_k)
    }

    // ── Pressure‑gradient impulses ─────────────────────────────────────

    /// Apply pressure‑gradient forces to gases, loose powders, liquids, and
    /// embers/sparks in active chunks.  Uses quarter‑cell vx_frac/vy_frac.
    pub(super) fn step_pressure_forces(&mut self) {
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
                        // Only apply to materials that respond to pressure.
                        if !self.pressure_responsive(m) {
                            continue;
                        }
                        if !cell_is_passable(m) && !m.is_fluid() {
                            continue;
                        }

                        let p_here = self.cell_pressure(i);

                        // Centered pressure gradients apply at most one impulse per axis.
                        let neighbor_pressure = |dx, dy| {
                            self.adj(x, y, dx, dy)
                                .map(|(nx, ny)| self.idx(nx, ny))
                                .filter(|&ni| {
                                    cell_is_passable(self.grid[ni]) || self.grid[ni].is_fluid()
                                })
                                .map(|ni| self.cell_pressure(ni))
                                .unwrap_or(p_here)
                        };
                        let horizontal_diff =
                            (neighbor_pressure(-1, 0).saturating_sub(neighbor_pressure(1, 0))) / 2;
                        let vertical_diff =
                            (neighbor_pressure(0, -1).saturating_sub(neighbor_pressure(0, 1))) / 2;

                        if horizontal_diff.abs() >= 384 {
                            let impulse = (horizontal_diff / PRESSURE_SCALE).clamp(-8, 8) as i8;
                            self.apply_horizontal_impulse(i, impulse, m);
                        }
                        if vertical_diff.abs() >= 384 {
                            let impulse = (vertical_diff / PRESSURE_SCALE).clamp(-8, 8) as i8;
                            self.apply_vertical_impulse(i, impulse, m);
                        }
                    }
                }
            }
        }
    }

    /// Whether a material responds to pressure‑gradient forces.
    fn pressure_responsive(&self, m: Material) -> bool {
        m.is_gas()
            || matches!(m, Fire | Smoke | Steam | Ember | FireworkSpark)
            || m.is_liquid()
            || matches!(m, Sand | BrokenGlass | Ash | Salt | Gunpowder | Coal)
    }

    /// Apply a horizontal impulse to `vx_frac` (quarter‑cell units).
    pub(super) fn apply_horizontal_impulse(&mut self, i: usize, impulse: i8, _m: Material) {
        let fixed = (self.vx[i] as i16) * (VELOCITY_SCALE as i16) + (self.vx_frac[i] as i16);
        let max_fixed = (MAX_VELOCITY as i16) * (VELOCITY_SCALE as i16);
        let new_fixed = (fixed + impulse as i16).clamp(-max_fixed, max_fixed);
        self.vx[i] = (new_fixed / VELOCITY_SCALE as i16) as i8;
        self.vx_frac[i] = (new_fixed % VELOCITY_SCALE as i16) as i8;
        self.activate_idx(i);
    }

    /// Apply a vertical impulse to `vy_frac` (quarter‑cell units).
    pub(super) fn apply_vertical_impulse(&mut self, i: usize, impulse: i8, _m: Material) {
        let fixed = (self.vy[i] as i16) * (VELOCITY_SCALE as i16) + (self.vy_frac[i] as i16);
        let max_fixed = (MAX_VELOCITY as i16) * (VELOCITY_SCALE as i16);
        let new_fixed = (fixed + impulse as i16).clamp(-max_fixed, max_fixed);
        self.vy[i] = (new_fixed / VELOCITY_SCALE as i16) as i8;
        self.vy_frac[i] = (new_fixed % VELOCITY_SCALE as i16) as i8;
        self.activate_idx(i);
    }

    /// Inject overpressure (extra air mass) from an explosion. Heat is applied by
    /// `explode` itself; this only drives the atmospheric pressure wave through
    /// passable cells with line of sight to the epicentre.
    pub(super) fn explode_atmos_effect(&mut self, x: usize, y: usize, profile: BlastProfile) {
        let radius = profile.radius;
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let dist2 = dx * dx + dy * dy;
                if dist2 > r2 {
                    continue;
                }
                let Some((tx, ty)) = self.adj(x, y, dx, dy) else {
                    continue;
                };
                if dist2 > 0 && !self.blast_has_line_of_sight(x, y, tx, ty) {
                    continue;
                }
                let ti = self.idx(tx, ty);
                if !cell_is_passable(self.grid[ti]) {
                    continue;
                }
                let strength = if dist2 == 0 {
                    1000
                } else {
                    let linear = (r2 - dist2) * 1000 / r2.max(1);
                    linear.max(350)
                };
                // profile.pressure is in ambient-mass units at the epicentre.
                let blast_mass = ((AMBIENT_AIR_MASS as i32 * profile.pressure as i32 * strength)
                    / 1000)
                    .max(AMBIENT_AIR_MASS as i32 / 4) as i16;
                let old_mass = self.air_mass[ti];
                let new_mass = clamp_mass(old_mass.saturating_add(blast_mass));
                let added_mass = new_mass - old_mass;
                self.air_mass[ti] = new_mass;
                let added_o2 =
                    (added_mass as i32 * AMBIENT_O2 as i32 / AMBIENT_AIR_MASS as i32) as i16;
                self.o2[ti] = clamp_mass(self.o2[ti].saturating_add(added_o2));
                self.activate_next(tx, ty);
            }
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Tests
// ───────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── One‑cell venting ───────────────────────────────────────────

    #[test]
    fn single_cell_vent_to_open_edge() {
        let mut w = World::new(3, 3);
        // Place a pocket of dense air at the top edge.
        let i = w.idx(1, 0);
        w.air_mass[i] = AMBIENT_AIR_MASS * 2;
        w.o2[i] = AMBIENT_O2 * 2;
        w.step_atmosphere();

        let mass = w.air_mass[w.idx(1, 0)];
        assert!(
            mass < AMBIENT_AIR_MASS * 2,
            "excess air at edge should vent (mass={mass})"
        );
        assert!(
            mass >= AMBIENT_AIR_MASS,
            "edge cell should not drop below ambient (mass={mass})"
        );
    }

    // ── Sealed gas retention ────────────────────────────────────────

    #[test]
    fn sealed_gas_retains_mass() {
        let mut w = World::new(5, 5);
        // Create a sealed box.
        for x in 0..5 {
            for y in 0..5 {
                let i = w.idx(x, y);
                w.grid[i] = Stone;
            }
        }
        // Open an interior cell.
        let interior = w.idx(2, 2);
        w.grid[interior] = Empty;
        w.air_mass[interior] = AMBIENT_AIR_MASS * 2; // overpressured
        w.o2[interior] = AMBIENT_O2 * 2;

        let mass_before = w.air_mass[interior];
        w.step_atmosphere();
        let mass_after = w.air_mass[interior];
        assert_eq!(mass_after, mass_before, "sealed gas should not change mass");
    }

    // ── Oxygen depletion and fire extinguishing ──────────────────────

    #[test]
    fn critical_oxygen_weakens_before_eventually_extinguishing_fire() {
        let mut w = World::new(3, 3);
        let fi = w.idx(1, 1);
        w.grid[fi] = Fire;
        w.life[fi] = 100;
        for x in 0..3 {
            for y in 0..3 {
                if !(x == 1 && y == 1) {
                    let i = w.idx(x, y);
                    w.grid[i] = Stone;
                }
            }
        }
        w.air_mass[fi] = AMBIENT_AIR_MASS;
        w.o2[fi] = 0;

        for _ in 0..20 {
            w.atmos_fire_burn(1, 1, fi);
        }
        assert_eq!(
            w.grid[fi], Fire,
            "critical oxygen should not kill fire immediately"
        );
        assert_eq!(
            w.fire_heat(fi),
            250,
            "critical oxygen should sharply reduce heat"
        );

        for _ in 0..30 {
            if w.grid[fi] != Fire {
                break;
            }
            w.atmos_fire_burn(1, 1, fi);
        }
        assert_eq!(
            w.grid[fi], Smoke,
            "sustained critical oxygen should extinguish fire"
        );
    }

    #[test]
    fn low_oxygen_reduces_heat_without_shortening_flame_life() {
        let mut w = World::new(3, 3);
        let fi = w.idx(1, 1);
        w.grid[fi] = Fire;
        w.life[fi] = 100;
        w.air_mass[fi] = AMBIENT_AIR_MASS;
        w.o2[fi] = 6;

        w.atmos_fire_burn(1, 1, fi);

        assert_eq!(w.grid[fi], Fire);
        assert_eq!(w.life[fi], 100);
        assert_eq!(w.fire_heat(fi), 500);
    }

    #[test]
    fn fire_consumes_oxygen_and_produces_exhaust() {
        let mut w = World::new(3, 3);
        let fi = w.idx(1, 1);
        w.grid[fi] = Fire;
        w.life[fi] = 100;
        let fuel = w.idx(2, 1);
        w.grid[fuel] = Wood;
        w.air_mass[fi] = AMBIENT_AIR_MASS;
        w.o2[fi] = AMBIENT_O2;
        w.exhaust[fi] = 0;

        w.atmos_fire_burn(1, 1, fi);

        assert!(
            w.o2[fi] < AMBIENT_O2,
            "fire should consume O₂ (was {AMBIENT_O2}, now {})",
            w.o2[fi]
        );
        assert!(w.exhaust[fi] > 0, "fire should produce exhaust");
    }

    // ── Ventilated oxygen replenishment ──────────────────────────────

    #[test]
    fn ventilated_o2_replenishes() {
        let mut w = World::new(3, 3);
        let fi = w.idx(1, 1);
        w.grid[fi] = Fire;
        w.life[fi] = 100;
        // Fire cell has low O₂, neighbours have ambient.
        w.air_mass[fi] = AMBIENT_AIR_MASS;
        w.o2[fi] = 1; // nearly depleted

        // draw O₂ from neighbours
        let neighbours_before: i16 = [(0i32, -1i32), (0, 1), (-1, 0), (1, 0)]
            .into_iter()
            .filter_map(|(dx, dy)| w.adj(1, 1, dx, dy))
            .map(|(nx, ny)| w.o2[w.idx(nx, ny)])
            .sum();
        let drawn = w.draw_o2_from_neighbors(1, 1, MAX_TRANSPORT);
        assert!(drawn > 0, "should draw O₂ from neighbours");
        assert!(w.o2[fi] > 1, "fire cell O₂ should increase");
        let neighbours_after: i16 = [(0i32, -1i32), (0, 1), (-1, 0), (1, 0)]
            .into_iter()
            .filter_map(|(dx, dy)| w.adj(1, 1, dx, dy))
            .map(|(nx, ny)| w.o2[w.idx(nx, ny)])
            .sum();
        assert_eq!(neighbours_before - neighbours_after, drawn);
    }

    #[test]
    fn ventilation_materially_extends_burning() {
        let mut sealed = World::new(3, 3);
        let si = sealed.idx(1, 1);
        sealed.grid[si] = Fire;
        sealed.life[si] = 100;
        sealed.o2[si] = 0;
        for y in 0..3 {
            for x in 0..3 {
                if (x, y) != (1, 1) {
                    let i = sealed.idx(x, y);
                    sealed.grid[i] = Stone;
                }
            }
        }

        let mut ventilated = World::new(3, 3);
        let vi = ventilated.idx(1, 1);
        ventilated.grid[vi] = Fire;
        ventilated.life[vi] = 100;
        ventilated.o2[vi] = 0;

        for tick in 0..50 {
            sealed.tick = tick;
            ventilated.tick = tick;
            if sealed.grid[si] == Fire {
                sealed.atmos_fire_burn(1, 1, si);
            }
            ventilated.atmos_fire_burn(1, 1, vi);
        }

        assert_eq!(sealed.grid[si], Smoke, "sealed fire should suffocate");
        assert_eq!(
            ventilated.grid[vi], Fire,
            "adjacent ventilated air should preserve the flame"
        );
        assert!(ventilated.life[vi] >= 90);
    }

    #[test]
    fn enclosed_wood_fire_spreads_before_oxygen_runs_low() {
        let mut w = World::new(7, 7);
        for x in 0..7 {
            w.paint(x, 0, Stone);
            w.paint(x, 6, Stone);
        }
        for y in 1..6 {
            w.paint(0, y, Stone);
            w.paint(6, y, Stone);
        }
        w.paint(3, 2, Wood);
        w.paint(3, 3, Fire);
        let fire = w.idx(3, 3);
        w.life[fire] = 100;

        for tick in 0..48 {
            w.tick = tick;
            w.atmos_fire_burn(3, 3, fire);
            assert_eq!(w.grid[fire], Fire, "enclosed flame died before spreading");
            let effective_temp = w.effective_temp(3, 2);
            if w.step_combustible(3, 2, effective_temp) {
                break;
            }
            w.step_transport_atmos();
        }

        assert_eq!(
            w.get(3, 2),
            Ember,
            "the enclosure's air reserve should sustain fire long enough to ignite wood"
        );
    }

    // ── Fuel vapor transport / ignition ─────────────────────────────

    #[test]
    fn fuel_vapor_ignites_adding_heat_and_overpressure() {
        let mut w = World::new(3, 3);
        let fi = w.idx(1, 1);
        w.grid[fi] = Fire;
        w.life[fi] = 100;
        w.air_mass[fi] = AMBIENT_AIR_MASS;
        w.o2[fi] = AMBIENT_O2;
        w.fuel_vapor[fi] = MAX_TRANSPORT * 2;
        let temp_before = w.temp[fi];
        let mass_before = w.air_mass[fi];

        w.atmos_fire_burn(1, 1, fi);

        assert!(
            w.fuel_vapor[fi] < MAX_TRANSPORT * 2,
            "fuel vapor should be consumed"
        );
        assert!(w.temp[fi] > temp_before, "vapor ignition should add heat");
        assert!(
            w.air_mass[fi] >= mass_before,
            "vapor ignition should not reduce air mass"
        );
    }

    #[test]
    fn oil_emits_fuel_vapor_when_hot() {
        let mut w = World::new(3, 3);
        let oi = w.idx(1, 1);
        w.grid[oi] = Oil;
        w.temp[oi] = 300; // hot enough
        // Make a passable neighbour.
        let ni = w.idx(1, 0);
        w.grid[ni] = Empty;

        w.atmos_oil_vapor(1, 1, oi);

        assert!(
            w.fuel_vapor[ni] > 0,
            "hot oil should emit fuel vapor to adjacent empty cell"
        );
    }

    // ── Transport conservation ───────────────────────────────────────

    #[test]
    fn equalize_species_conserves_mass_and_cell_capacity() {
        let mut w = World::new(2, 1);
        let a = w.idx(0, 0);
        let b = w.idx(1, 0);
        w.air_mass[a] = 1;
        w.air_mass[b] = 1;
        w.o2[a] = 1;
        w.o2[b] = 0;
        w.exhaust[a] = 0;
        w.exhaust[b] = 1;

        w.equalize_species(a, b);

        assert_eq!(w.o2[a] + w.o2[b], 1);
        assert_eq!(w.exhaust[a] + w.exhaust[b], 1);
        assert_eq!(w.fuel_vapor[a] + w.fuel_vapor[b], 0);
        assert!(species_total(w.o2[a], w.exhaust[a], w.fuel_vapor[a]) <= w.air_mass[a]);
        assert!(species_total(w.o2[b], w.exhaust[b], w.fuel_vapor[b]) <= w.air_mass[b]);
    }

    #[test]
    fn transport_does_not_overshoot_one_unit_gradient() {
        let mut w = World::new(2, 1);
        let a = w.idx(0, 0);
        let b = w.idx(1, 0);
        w.air_mass[a] = 65;
        w.air_mass[b] = 64;

        assert!(!w.transport_pair(a, b));
        assert_eq!((w.air_mass[a], w.air_mass[b]), (65, 64));
    }

    #[test]
    fn ambient_vent_restores_oxygen_ratio_and_reactivates_chunk() {
        let mut w = World::new(1, 1);
        let i = w.idx(0, 0);
        w.air_mass[i] = 0;
        w.o2[i] = 0;
        w.next_active_chunks.fill(false);

        for _ in 0..8 {
            w.vent_to_ambient(i);
        }

        assert_eq!(w.air_mass[i], AMBIENT_AIR_MASS);
        assert_eq!(w.o2[i], AMBIENT_O2);
        assert!(w.next_active_chunks.iter().any(|&active| active));
    }

    #[test]
    fn hot_air_pressure_drives_flow_and_carries_heat() {
        let mut w = World::new(2, 1);
        let hot = w.idx(0, 0);
        let cool = w.idx(1, 0);
        w.air_mass[hot] = AMBIENT_AIR_MASS;
        w.air_mass[cool] = AMBIENT_AIR_MASS;
        w.temp[hot] = 500;
        w.temp[cool] = AMBIENT_TEMP;

        assert!(w.transport_pair(hot, cool));
        assert!(w.air_mass[hot] < AMBIENT_AIR_MASS);
        assert!(w.air_mass[cool] > AMBIENT_AIR_MASS);
        assert!(w.temp[cool] > AMBIENT_TEMP, "moving air should carry heat");
    }

    #[test]
    fn vertical_transport_settles_cool_heavy_exhaust() {
        let mut w = World::new(1, 2);
        let upper = w.idx(0, 0);
        let lower = w.idx(0, 1);
        w.o2[upper] = 0;
        w.o2[lower] = 0;
        w.exhaust[upper] = 4;
        w.exhaust[lower] = 0;

        assert!(w.transport_vertical_pair(upper, lower));

        assert_eq!(w.exhaust[upper] + w.exhaust[lower], 4);
        assert!(
            w.exhaust[lower] > w.exhaust[upper],
            "expected cool exhaust to settle, got {:?}",
            (w.exhaust[upper], w.exhaust[lower])
        );
    }

    #[test]
    fn vertical_transport_lifts_hot_exhaust() {
        let mut w = World::new(1, 2);
        let upper = w.idx(0, 0);
        let lower = w.idx(0, 1);
        w.o2[upper] = 0;
        w.o2[lower] = 0;
        w.exhaust[upper] = 0;
        w.exhaust[lower] = 4;
        w.temp[lower] = 500;

        assert!(w.transport_vertical_pair(upper, lower));

        assert_eq!(w.exhaust[upper] + w.exhaust[lower], 4);
        assert!(
            w.exhaust[upper] > w.exhaust[lower],
            "expected hot exhaust to rise, got {:?}",
            (w.exhaust[upper], w.exhaust[lower])
        );
    }

    #[test]
    fn normal_pressure_edge_exchange_clears_exhaust() {
        let mut w = World::new(1, 1);
        let i = w.idx(0, 0);
        w.o2[i] = 0;
        w.exhaust[i] = 8;
        let mass = w.air_mass[i];

        for _ in 0..8 {
            w.vent_to_ambient(i);
        }

        assert_eq!(w.air_mass[i], mass);
        assert_eq!(w.exhaust[i], 0);
        assert!(w.o2[i] > 0);
    }

    // ── Disabled state freeze / reset ───────────────────────────────

    #[test]
    fn disabled_atmos_does_not_change_state() {
        let mut w = World::new(3, 3);
        let i = w.idx(1, 1);
        w.air_mass[i] = 42;
        w.o2[i] = 7;
        w.atmos_enabled = false;
        let prev_mass = w.air_mass[i];
        let prev_o2 = w.o2[i];

        // Full world step won't call atmosphere.
        w.step();

        assert_eq!(w.air_mass[i], prev_mass, "air mass unchanged when disabled");
        assert_eq!(w.o2[i], prev_o2, "O₂ unchanged when disabled");
    }

    #[test]
    fn reset_atmosphere_clears_all_cells() {
        let mut w = World::new(3, 3);
        let i = w.idx(1, 1);
        w.air_mass[i] = 999;
        w.o2[i] = 999;
        w.exhaust[i] = 50;
        w.fuel_vapor[i] = 30;

        w.reset_atmosphere();

        for i in 0..w.grid.len() {
            assert_eq!(w.air_mass[i], AMBIENT_AIR_MASS);
            assert_eq!(w.o2[i], AMBIENT_O2);
            assert_eq!(w.exhaust[i], 0);
            assert_eq!(w.fuel_vapor[i], 0);
        }
    }

    // ── Pressure impulse ────────────────────────────────────────────

    #[test]
    fn pressure_gradient_applies_impulse() {
        let mut w = World::new(3, 3);
        // High pressure cell next to ambient.
        let hi = w.idx(1, 1);
        w.grid[hi] = Empty;
        w.air_mass[hi] = AMBIENT_AIR_MASS * 3;
        w.temp[hi] = 900; // hot

        let ri = w.idx(2, 1);
        w.grid[ri] = Smoke;
        w.life[ri] = 100;
        w.air_mass[ri] = AMBIENT_AIR_MASS;

        let velocity_before = (w.vx[ri], w.vx_frac[ri]);
        w.activate_now(2, 1);
        w.step_pressure_forces();
        assert_ne!(
            (w.vx[ri], w.vx_frac[ri]),
            velocity_before,
            "pressure gradient should apply some impulse"
        );
    }

    #[test]
    fn smoke_follows_draft_through_one_cell_opening() {
        let mut w = World::new(7, 5);
        for y in 0..w.height {
            if y != 2 {
                w.paint(3, y, Metal);
            }
        }
        w.paint(2, 2, Smoke);
        let smoke = w.idx(2, 2);
        w.life[smoke] = 100;
        w.temp[smoke] = 500;
        w.moved_tick[smoke] = u64::MAX;

        let pressure_source = w.idx(1, 2);
        w.air_mass[pressure_source] = MAX_AIR_MASS;
        w.temp[pressure_source] = 900;
        w.activate_now(2, 2);

        w.step();

        assert!(
            (4..w.width).any(|x| w.get(x, 2) == Smoke),
            "draft should carry smoke through the one-cell opening; smoke={:?}, pressures={:?}",
            w.grid
                .iter()
                .enumerate()
                .filter(|(_, material)| **material == Smoke)
                .map(|(i, _)| (i % w.width, i / w.width, w.vx[i], w.vx_frac[i]))
                .collect::<Vec<_>>(),
            (0..w.width)
                .map(|x| w.cell_pressure(w.idx(x, 2)))
                .collect::<Vec<_>>()
        );
    }
}
