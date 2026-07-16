use super::*;

impl World {
    /// Diffuse heat through active chunks. Sources clamp; other cells equalize
    /// with neighbours and slowly return toward ambient.
    pub(super) fn step_heat(&mut self) {
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
                        if let Some(mut src) = m.heat_source_temp() {
                            if m == Fire {
                                src = self.fire_heat(i);
                            }
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
                        let ambient_delta = AMBIENT_TEMP as i32 - next;
                        next += if ambient_delta.abs() < 48 {
                            ambient_delta.signum()
                        } else {
                            ambient_delta / 48
                        };
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
    pub(super) fn effective_temp(&self, x: usize, y: usize) -> i16 {
        let i = self.idx(x, y);
        let mut t = self.temp[i];
        for n in self.n8(x, y).into_iter().flatten() {
            let ni = self.idx(n.0, n.1);
            let m = self.grid[ni];
            if let Some(mut src) = m.heat_source_temp() {
                if m == Fire {
                    src = self.fire_heat(ni);
                }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heat_diffusion_is_symmetric_and_stays_within_initial_bounds() {
        let mut world = World::new(3, 3);
        let center = world.idx(1, 1);
        world.temp[center] = 500;
        world.active_chunks.fill(true);

        world.step_heat();

        let neighbors = [
            world.temp[world.idx(1, 0)],
            world.temp[world.idx(0, 1)],
            world.temp[world.idx(2, 1)],
            world.temp[world.idx(1, 2)],
        ];
        assert!(neighbors.iter().all(|&temp| temp == neighbors[0]));
        assert!(neighbors[0] > AMBIENT_TEMP);
        assert!(world.temp[center] < 500);
        assert!(
            world
                .temp
                .iter()
                .all(|&temp| (AMBIENT_TEMP..=500).contains(&temp))
        );
    }

    #[test]
    fn heat_sources_remain_clamped_during_diffusion() {
        let mut world = World::new(3, 3);
        world.paint(1, 1, Lava);
        world.temp.fill(-100);
        world.active_chunks.fill(true);

        world.step_heat();

        assert_eq!(
            world.temp[world.idx(1, 1)],
            Lava.heat_source_temp().unwrap()
        );
    }

    #[test]
    fn near_ambient_temperature_converges_instead_of_stalling() {
        let mut world = World::new(1, 1);
        world.temp[0] = AMBIENT_TEMP + 1;
        world.active_chunks.fill(true);

        world.step_heat();

        assert_eq!(world.temp[0], AMBIENT_TEMP);
    }
}
