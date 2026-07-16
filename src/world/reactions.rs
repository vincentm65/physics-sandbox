use super::*;

impl World {
    /// Central material chemistry. Returns true when the cell at `(x, y)` was
    /// consumed or replaced and should stop further stepping this tick.
    pub(super) fn react_cell(&mut self, x: usize, y: usize) -> bool {
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

    pub(super) fn react_liquid_nitrogen(&mut self, x: usize, y: usize) -> bool {
        let i = self.idx(x, y);
        for n in self.n4(x, y).into_iter().flatten() {
            let ni = self.idx(n.0, n.1);
            match self.grid[ni] {
                Water => self.put(ni, Ice, 0),
                Fire | Ember | Lava => {
                    self.put(ni, if self.grid[ni] == Lava { Stone } else { Smoke }, 0);
                    self.put_rand_range(i, Steam, STEAM_LIFE_MIN, STEAM_LIFE_MAX);
                    return true;
                }
                // Hot glass meets cryogenic quench → shatter.
                Glass if self.temp[ni] >= 200 => {
                    self.put(ni, BrokenGlass, 0);
                }
                // Extreme-cold thermal shock: hot stone/concrete/metal adjacent to
                // liquid nitrogen may crack from rapid contraction.
                Stone if self.temp[ni] >= 400 && self.chance(n.0, n.1, 0xA5, 80) => {
                    self.put(ni, Sand, 0);
                }
                Concrete if self.temp[ni] >= 500 && self.chance(n.0, n.1, 0xA6, 60) => {
                    self.put(ni, Sand, 0);
                }
                Metal if self.temp[ni] >= 600 && self.chance(n.0, n.1, 0xA7, 20) => {
                    self.put(ni, Empty, 0);
                }
                _ => {}
            }
        }
        false
    }

    pub(super) fn react_lava(&mut self, x: usize, y: usize) -> bool {
        let mut solidified = false;
        for n in self.n4(x, y) {
            let Some((nx, ny)) = n else {
                continue;
            };
            let ni = self.idx(nx, ny);
            match self.grid[ni] {
                Water => {
                    self.put_rand_range(ni, Steam, STEAM_LIFE_MIN, STEAM_LIFE_MAX);
                    solidified = true;
                }
                // Sand/shards already softened by heat can be absorbed into the flow.
                // Stone/concrete use the slower heat-soak melt path instead.
                Sand | BrokenGlass if self.chance(nx, ny, 0x72, 40) => {
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

    pub(super) fn react_acid(&mut self, x: usize, y: usize) -> bool {
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
    /// Uses a single probability roll for responsiveness, and emits a short-lived
    /// Smoke puff for visible feedback when salt dissolves.
    pub(super) fn react_salt(&mut self, x: usize, y: usize) -> bool {
        let i = self.idx(x, y);
        for n in self.n4(x, y).into_iter().flatten() {
            let ni = self.idx(n.0, n.1);
            match self.grid[ni] {
                Water if self.chance(x, y, 0x90, 40) => {
                    // dissolve: remove salt, leave the water cell untouched.
                    // Emit a brief white puff at the salt location for feedback.
                    self.put(i, Smoke, 12);
                    return true;
                }
                Ice if self.chance(n.0, n.1, 0x93, 120) => {
                    // brine lowers freeze point: ice thaws
                    self.put(ni, Water, 0);
                    if self.chance(n.0, n.1, 0x94, 200) {
                        self.put(i, Smoke, 12);
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Mercury slowly amalgamates (eats) adjacent metal.
    pub(super) fn react_mercury(&mut self, x: usize, y: usize) -> bool {
        for n in self.n4(x, y).into_iter().flatten() {
            let ni = self.idx(n.0, n.1);
            if self.grid[ni] == Metal && self.chance(n.0, n.1, 0x95, 15) {
                self.put(ni, Empty, 0);
                return false;
            }
        }
        false
    }

    pub(super) fn step_plant(&mut self, x: usize, y: usize) {
        // Acid is handled by react_acid when the acid cell steps; plants only grow here.
        // Plant grows when adjacent to Water (water is NOT consumed).
        for n in self.n4(x, y) {
            let Some((nx, ny)) = n else { continue };
            let ni = self.idx(nx, ny);
            if self.grid[ni] == Water && self.chance(nx, ny, 0xA2, 25) {
                // Find an empty neighbor to grow into (not the water cell itself).
                for nn in self.n4(x, y) {
                    let Some((nnx, nny)) = nn else { continue };
                    let nni = self.idx(nnx, nny);
                    if self.grid[nni] == Empty {
                        self.put(nni, Plant, 0);
                        return;
                    }
                }
            }
        }
    }

    pub(super) fn step_ice(&mut self, x: usize, y: usize) {
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
            }
        }

        // Salt brine is handled when salt steps; acid etching when acid steps.
    }

    pub(super) fn react_water(&mut self, x: usize, y: usize) -> bool {
        let i = self.idx(x, y);
        let heat = self.effective_temp(x, y);
        // freeze when very cold (LN2 diffusion / contact)
        if heat < 0 && self.chance(x, y, 0x81, 200) {
            self.put(i, Ice, 0);
            return true;
        }
        // Thermal-shock quench before boil: hot glass would otherwise raise
        // effective_temp enough for water to become steam and return without
        // ever reaching the shatter branch.
        for n in self.n4(x, y) {
            let Some((nx, ny)) = n else {
                continue;
            };
            let ni = self.idx(nx, ny);
            if self.grid[ni] == Glass && self.temp[ni] >= 300 {
                self.put(ni, BrokenGlass, 0);
            }
        }
        // Temperature-driven boil: heat soak is enough; contact with fire/lava
        // is no longer required (those paths still help via effective_temp).
        if heat >= 100 {
            // Base chance is high enough that sustained boiling-point water
            // converts within a few dozen ticks even after ambient bleed.
            let chance = (((heat as u32) - 100).min(500) / 2 + 150).min(600);
            if self.chance(x, y, 0x83, chance) {
                self.put_rand_range(i, Steam, STEAM_LIFE_MIN, STEAM_LIFE_MAX);
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
                    self.put_rand_range(i, Steam, STEAM_LIFE_MIN, STEAM_LIFE_MAX);
                    return true;
                }
                Lava => {
                    // water side of lava+water: boil; lava solidifies when it steps
                    self.put_rand_range(i, Steam, STEAM_LIFE_MIN, STEAM_LIFE_MAX);
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Emit a continuous column of water beneath the faucet. The cell stays
    /// active every tick so the stream does not die when the world settles.
    pub(super) fn step_faucet(&mut self, x: usize, y: usize) {
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

    pub(super) fn step_drain(&mut self, x: usize, y: usize) {
        for (nx, ny) in self.n8(x, y).into_iter().flatten() {
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
