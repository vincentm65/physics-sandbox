use super::core::StructuralMove;
use super::*;

impl World {
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
        _material: Material,
        component: &[usize],
        in_component: &[bool],
        dx: i32,
        dy: i32,
    ) {
        let mut ordered = std::mem::take(&mut self.structural_ordered);
        ordered.clear();
        ordered.extend_from_slice(component);
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

        let mut moves = std::mem::take(&mut self.structural_moves);
        moves.clear();
        let mut displaced = std::mem::take(&mut self.structural_displaced);
        displaced.clear();
        for &i in &ordered {
            let x = i % self.width;
            let y = i / self.width;
            let (tx, ty) = self
                .adj(x, y, dx, dy)
                .expect("prechecked structural translation");
            let ti = self.idx(tx, ty);
            moves.push(StructuralMove {
                source: i,
                target: ti,
                state: self.snapshot_state(i),
            });
            if !in_component[ti] && self.grid[ti] != Empty {
                // Return the displaced state to the trailing vacancy on the same
                // translation line, preserving both row and column correspondence.
                let mut vacancy = i;
                loop {
                    let vx = vacancy % self.width;
                    let vy = vacancy / self.width;
                    let Some((px, py)) = self.adj(vx, vy, -dx, -dy) else {
                        break;
                    };
                    let pi = self.idx(px, py);
                    if !in_component[pi] {
                        break;
                    }
                    vacancy = pi;
                }
                displaced.push((vacancy, self.snapshot_state(ti)));
            }
        }

        for movement in &moves {
            self.clear_cell_state(movement.source);
            self.moved_tick[movement.source] = self.tick;
            self.activate_idx(movement.source);
        }
        for movement in &moves {
            self.set_cell_state(movement.target, movement.state);
            self.moved_tick[movement.target] = self.tick;
            self.activate_idx(movement.target);
        }
        for &(i, state) in &displaced {
            self.set_cell_state(i, state);
        }

        ordered.clear();
        moves.clear();
        displaced.clear();
        self.structural_ordered = ordered;
        self.structural_moves = moves;
        self.structural_displaced = displaced;
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
            self.vx[i] = 0;
            self.vy[i] = 0;
            self.vy_frac[i] = 0;
            self.y_frac[i] = 0;
            self.moved_tick[i] = self.tick;
            self.activate_idx(i);
        }
    }

    /// Move unsupported connected structural islands down one cell. Glass turns
    /// into broken glass when an island that fell on the previous tick hits support.
    pub(super) fn step_structural_components(&mut self) {
        let mut stack = std::mem::take(&mut self.structural_stack);
        stack.clear();
        let mut component = std::mem::take(&mut self.structural_component);
        component.clear();
        let mut in_component = std::mem::take(&mut self.structural_membership);
        in_component.fill(false);

        for material in [Wood, Stone, Glass] {
            self.structural_generation = self.structural_generation.wrapping_add(1);
            if self.structural_generation == 0 {
                self.structural_seen.fill(0);
                self.structural_generation = 1;
            }
            let generation = self.structural_generation;
            for y in (0..self.height).rev() {
                for x in 0..self.width {
                    let start = self.idx(x, y);
                    if self.grid[start] != material
                        || self.moved_tick[start] == self.tick
                        || self.structural_seen[start] == generation
                    {
                        continue;
                    }

                    stack.clear();
                    component.clear();
                    stack.push(start);
                    self.structural_seen[start] = generation;
                    while let Some(i) = stack.pop() {
                        component.push(i);
                        let cx = i % self.width;
                        let cy = i / self.width;
                        for n in self.n4(cx, cy) {
                            let Some((nx, ny)) = n else { continue };
                            let ni = self.idx(nx, ny);
                            if self.grid[ni] == material
                                && self.moved_tick[ni] != self.tick
                                && self.structural_seen[ni] != generation
                            {
                                self.structural_seen[ni] = generation;
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

        stack.clear();
        component.clear();
        in_component.fill(false);
        self.structural_stack = stack;
        self.structural_component = component;
        self.structural_membership = in_component;
    }
}
