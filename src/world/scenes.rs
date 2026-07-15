use super::*;

fn fill_rect(world: &mut World, x0: usize, y0: usize, x1: usize, y1: usize, m: Material) {
    let x1 = x1.min(world.width);
    let y1 = y1.min(world.height);
    for y in y0.min(world.height)..y1 {
        for x in x0.min(world.width)..x1 {
            world.paint(x, y, m);
        }
    }
}

fn fill_column(world: &mut World, x: usize, y0: usize, y1: usize, width: usize, m: Material) {
    if y1 > y0 {
        fill_rect(world, x, y0, x.saturating_add(width), y1, m);
    }
}

/// Thick diagonal stroke from (x0,y0) to (x1,y1).
fn fill_line(
    world: &mut World,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    thickness: usize,
    m: Material,
) {
    let dx = x1 as i32 - x0 as i32;
    let dy = y1 as i32 - y0 as i32;
    let steps = dx.abs().max(dy.abs()).max(1);
    let t = thickness.max(1) as i32;
    for i in 0..=steps {
        let x = x0 as i32 + dx * i / steps;
        let y = y0 as i32 + dy * i / steps;
        for oy in 0..t {
            for ox in 0..t {
                let px = x + ox - t / 2;
                let py = y + oy;
                if px >= 0 && py >= 0 {
                    world.paint(px as usize, py as usize, m);
                }
            }
        }
    }
}

/// Solid stair wedge that cannot float.
///
/// `y_top` / `y_bot` are the sky-side faces of the upper and lower decks
/// (smaller y = higher). The wedge fills from just under the upper deck down
/// to the lower deck and is pinned by side stringers, so every tread is
/// backed by structure all the way to a supported landing.
fn solid_stairs(world: &mut World, x0: usize, x1: usize, y_top: usize, y_bot: usize, m: Material) {
    if x1 <= x0 + 3 || y_bot <= y_top + 2 {
        return;
    }
    // Climb through the open well: start just below upper deck, land on lower deck.
    let y_high = y_top + 1; // first empty row under upper deck
    let y_low = y_bot; // sky-side face of lower deck (landing)
    if y_low <= y_high {
        return;
    }
    let rise = y_low - y_high;
    let run = x1 - x0;

    // Continuous side stringers tying both landings.
    fill_column(world, x0, y_top, y_bot + 2, 1, m);
    fill_column(world, x1.saturating_sub(1), y_top, y_bot + 2, 1, m);

    // Solid wedge: each row is a tread whose front advances with height.
    // Material always spans back to the left stringer, so nothing floats.
    for i in 0..=rise {
        let y = y_low - i;
        if y < y_high {
            break;
        }
        let advance = (i * run) / rise.max(1);
        let tread_x1 = (x0 + advance + 3).min(x1);
        fill_rect(world, x0, y, tread_x1, y + 1, m);
    }

    // Landing lips glued into both floor decks.
    fill_rect(world, x0.saturating_sub(2), y_bot, x0 + 3, y_bot + 2, m);
    fill_rect(
        world,
        x1.saturating_sub(3),
        y_top,
        (x1 + 2).min(world.width),
        y_top + 2,
        m,
    );
}

fn plant_tree(world: &mut World, base_x: usize, grade: usize, trunk_h: usize, canopy_r: usize) {
    if grade <= trunk_h || base_x + 2 >= world.width {
        return;
    }
    let trunk_top = grade - trunk_h;
    fill_rect(world, base_x, trunk_top, base_x + 2, grade + 1, Wood);
    let cx = base_x as i32 + 1;
    let cy = trunk_top as i32 + 1;
    let r = canopy_r as i32;
    for dy in -r..=r {
        for dx in -r..=(r + 1) {
            if dx * dx + dy * dy <= r * r + r {
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && py >= 0 {
                    world.paint(px as usize, py as usize, Plant);
                }
            }
        }
    }
}

/// Large multi-bay timber house: basement, two storeys, attic, framed roof, yard trees.
pub(super) fn seed_house(world: &mut World) {
    let (w, h) = (world.width, world.height);
    if w < 48 || h < 28 {
        return;
    }

    // --- proportions that always fit the terminal ----------------------------
    // y grows downward. Stack from grade up; roof takes whatever remains.
    let top_air = 2usize;
    let grade = h.saturating_sub(3);
    if grade <= top_air + 16 {
        return;
    }
    let usable = grade - top_air;

    let floor_t = 2usize;
    let wall_t = 3usize;
    let post_w = 2usize;
    let eaves = (w / 40).clamp(3, 8);

    // Clear heights (empty room) — fractions of usable, no mins that overflow.
    // Reserve ~18% for roof, rest split across bas + 2 storeys + attic + 3 decks.
    let roof_budget = (usable * 18 / 100).max(4);
    let live = usable.saturating_sub(roof_budget);
    // 3 floor decks + remaining clear space across 4 levels (bas, gnd, up, attic).
    let deck_total = floor_t * 3;
    let clear_pool = live.saturating_sub(deck_total);
    // Weights: bas 3, gnd 4, up 4, attic 2  (sum 13)
    let bas_clear = (clear_pool * 3 / 13).max(3);
    let storey_clear = (clear_pool * 4 / 13).max(4);
    let attic_clear = clear_pool
        .saturating_sub(bas_clear + storey_clear * 2)
        .max(3);

    // Deck sky-side y (smaller y = higher).
    let y_ground = grade - bas_clear; // ground-floor deck
    let y_upper = y_ground - floor_t - storey_clear; // upper-floor deck
    let y_attic = y_upper - floor_t - storey_clear; // attic deck / wall plate
    if y_attic <= top_air + 4 {
        return;
    }
    let y_plate = y_attic;
    let roof_rise = y_plate.saturating_sub(top_air).min(roof_budget + 2).max(4);
    let y_ridge = y_plate.saturating_sub(roof_rise);
    if y_ridge < top_air {
        return;
    }

    // Room vertical spans (empty cells):
    //   basement:  [y_ground + floor_t , grade)
    //   ground:    [y_upper  + floor_t , y_ground)
    //   upper:     [y_attic  + floor_t , y_upper)
    //   attic:     [y_plate - knee…    , y_attic) under rafters
    let bas_ceil = y_ground + floor_t;
    let gnd_ceil = y_upper + floor_t;
    let up_ceil = y_attic + floor_t;

    // House width ~78% of terminal.
    let yard = (w * 11 / 100).clamp(6, 22);
    let left = yard;
    let right = w - yard;
    if right <= left + 36 {
        return;
    }
    let center = (left + right) / 2;
    let house_w = right - left;

    let n_bays = if house_w >= 90 {
        5
    } else if house_w >= 60 {
        4
    } else {
        3
    };
    let bay = house_w / n_bays;

    // Support lines: exterior walls + interior posts at every bay.
    let mut posts: Vec<usize> = Vec::with_capacity(n_bays + 1);
    for i in 0..n_bays {
        posts.push(left + i * bay);
    }
    posts.push(right - wall_t);

    // Stair well: one full interior bay near center.
    let stair_bay = (n_bays / 2).min(n_bays - 1).max(1);
    let stair_x0 = posts[stair_bay] + post_w;
    let stair_x1 = posts[stair_bay + 1];
    if stair_x1 <= stair_x0 + 5 {
        return;
    }

    // Chimney stack against the right exterior wall.
    let chim_w = 5usize;
    let chim_x0 = right - wall_t;
    let chim_x1 = (chim_x0 + chim_w).min(w.saturating_sub(1));
    let flue_x0 = chim_x0 + 1;
    let flue_x1 = chim_x1.saturating_sub(1);

    // --- terrain & foundation ----------------------------------------------
    fill_rect(world, 0, grade, w, h, Stone);
    for x in 0..w {
        if x % 7 == 0 {
            world.paint(x, grade, Sand);
        } else if x % 11 == 0 && grade + 1 < h {
            world.paint(x, grade + 1, Coal);
        }
    }

    let foot_pad = (bay / 4).clamp(3, 8);
    let foot_left = left.saturating_sub(foot_pad);
    let foot_right = (right + foot_pad).min(w);

    // Foundation mass, then carve basement volume under the ground deck.
    fill_rect(world, foot_left, y_ground, foot_right, grade + 1, Concrete);
    fill_rect(world, left + wall_t, bas_ceil, right - wall_t, grade, Empty);
    // Basement slab.
    fill_rect(world, left, grade - 1, right, grade + 1, Concrete);

    // --- shell: exterior walls + continuous posts --------------------------
    // Timber from wall plate down to ground deck; concrete stem in basement.
    fill_column(world, left, y_plate, y_ground + floor_t, wall_t, Wood);
    fill_column(
        world,
        right - wall_t,
        y_plate,
        y_ground + floor_t,
        wall_t,
        Wood,
    );
    fill_column(world, left, y_ground, grade, wall_t, Concrete);
    fill_column(world, right - wall_t, y_ground, grade, wall_t, Concrete);

    for &px in posts.iter().skip(1).take(n_bays - 1) {
        fill_column(world, px, y_plate, y_ground + floor_t, post_w, Wood);
        fill_column(world, px, y_ground, grade, post_w, Concrete);
    }

    // --- floor decks with stair wells --------------------------------------
    for &fy in &[y_ground, y_upper] {
        fill_rect(world, left, fy, right, fy + floor_t, Wood);
        fill_rect(world, stair_x0, fy, stair_x1, fy + floor_t, Empty);
    }
    // Attic deck with narrower hatch at the top of the stair wedge.
    fill_rect(world, left, y_attic, right, y_attic + floor_t, Wood);
    let hatch_x0 = stair_x1.saturating_sub((bay / 3).max(5)).max(stair_x0 + 2);
    let hatch_x1 = stair_x1;
    fill_rect(world, hatch_x0, y_attic, hatch_x1, y_attic + floor_t, Empty);

    // Re-assert posts/walls through decks so openings don't cut supports.
    fill_column(world, left, y_plate, y_ground + floor_t, wall_t, Wood);
    fill_column(
        world,
        right - wall_t,
        y_plate,
        y_ground + floor_t,
        wall_t,
        Wood,
    );
    for &px in posts.iter().skip(1).take(n_bays - 1) {
        fill_column(world, px, y_plate, y_ground + floor_t, post_w, Wood);
        fill_column(world, px, y_ground, grade, post_w, Concrete);
    }

    // --- solid stairs (floor-to-floor, stringered) --------------------------
    // Basement → ground.
    solid_stairs(world, stair_x0, stair_x1, y_ground, grade - 1, Wood);
    // Ground → upper.
    solid_stairs(world, stair_x0, stair_x1, y_upper, y_ground, Wood);
    // Upper → attic via hatch.
    solid_stairs(world, hatch_x0, hatch_x1, y_attic, y_upper, Wood);

    // Landing lips at each deck so the well edges stay tied in.
    for &fy in &[y_ground, y_upper, y_attic] {
        fill_rect(
            world,
            stair_x0.saturating_sub(2),
            fy,
            stair_x0 + 1,
            fy + floor_t,
            Wood,
        );
        fill_rect(
            world,
            stair_x1.saturating_sub(1),
            fy,
            (stair_x1 + 2).min(right),
            fy + floor_t,
            Wood,
        );
    }
    // Keep the attic hatch open after lip repair, then re-seat the attic flight.
    fill_rect(
        world,
        hatch_x0 + 1,
        y_attic,
        hatch_x1.saturating_sub(1),
        y_attic + floor_t,
        Empty,
    );
    solid_stairs(world, hatch_x0, hatch_x1, y_attic, y_upper, Wood);

    // --- interior partitions with door openings ----------------------------
    let door_h = (storey_clear * 3 / 5).clamp(3, 12);
    for (i, &px) in posts.iter().enumerate().skip(1).take(n_bays - 1) {
        if i == stair_bay {
            continue;
        }
        // Ground storey wall + door (room between gnd_ceil and y_ground).
        fill_column(world, px, gnd_ceil, y_ground, 1, Wood);
        let d0 = y_ground.saturating_sub(door_h);
        if d0 >= gnd_ceil {
            fill_rect(world, px, d0, px + 1, y_ground, Empty);
        }
        // Upper storey wall + door.
        fill_column(world, px, up_ceil, y_upper, 1, Wood);
        let d1 = y_upper.saturating_sub(door_h);
        if d1 >= up_ceil {
            fill_rect(world, px, d1, px + 1, y_upper, Empty);
        }
    }

    // --- roof framing ------------------------------------------------------
    let eave_l = left.saturating_sub(eaves);
    let eave_r = (right + eaves).min(w - 1);
    fill_rect(world, eave_l, y_plate, eave_r, y_plate + floor_t, Wood);

    fill_line(world, eave_l, y_plate, center, y_ridge, 3, Wood);
    fill_line(world, eave_r, y_plate, center, y_ridge, 3, Wood);
    let mid_y = y_ridge + roof_rise / 3;
    fill_line(
        world,
        left,
        y_plate,
        center.saturating_sub(bay / 4),
        mid_y,
        2,
        Wood,
    );
    fill_line(world, right - 1, y_plate, center + bay / 4, mid_y, 2, Wood);
    fill_rect(world, center - 3, y_ridge, center + 4, y_ridge + 2, Wood);
    fill_column(world, center - 1, y_ridge + 2, y_plate, 2, Wood);
    if bay >= 6 {
        fill_column(
            world,
            center.saturating_sub(bay / 3),
            mid_y,
            y_plate,
            2,
            Wood,
        );
        fill_column(world, center + bay / 3, mid_y, y_plate, 2, Wood);
    }

    let collar_y = y_ridge + roof_rise * 2 / 5;
    if collar_y + 1 < y_plate {
        fill_rect(
            world,
            center.saturating_sub(house_w / 3),
            collar_y,
            (center + house_w / 3).min(right),
            collar_y + 1,
            Wood,
        );
        let collar2 = y_ridge + roof_rise * 3 / 5;
        if collar2 > collar_y + 2 && collar2 + 1 < y_plate {
            fill_rect(
                world,
                center.saturating_sub(house_w * 2 / 5),
                collar2,
                (center + house_w * 2 / 5).min(right),
                collar2 + 1,
                Wood,
            );
        }
    }

    // Knee walls at outer attic bays.
    let knee_h = (attic_clear / 2).max(2).min(attic_clear);
    if posts.len() > 2 {
        fill_column(
            world,
            posts[1],
            y_plate.saturating_sub(knee_h),
            y_plate,
            post_w,
            Wood,
        );
        fill_column(
            world,
            posts[posts.len() - 2],
            y_plate.saturating_sub(knee_h),
            y_plate,
            post_w,
            Wood,
        );
    }

    // Dormer on left roof plane — glass only inset in the dormer face.
    let dorm_cx = left + bay;
    let dorm_w = (bay / 2).clamp(6, 14);
    let dorm_x0 = dorm_cx.saturating_sub(dorm_w / 2);
    let dorm_x1 = dorm_x0 + dorm_w;
    let dorm_sill = y_plate.saturating_sub((attic_clear * 2 / 3).max(3));
    let dorm_peak = dorm_sill.saturating_sub((roof_rise / 3).max(3));
    if dorm_peak + 2 < dorm_sill && dorm_x1 > dorm_x0 + 4 {
        fill_column(world, dorm_x0, dorm_peak, y_plate, 2, Wood);
        fill_column(
            world,
            dorm_x1.saturating_sub(2),
            dorm_peak,
            y_plate,
            2,
            Wood,
        );
        fill_rect(world, dorm_x0, dorm_sill, dorm_x1, dorm_sill + 1, Wood);
        let dorm_mid = (dorm_x0 + dorm_x1) / 2;
        fill_line(world, dorm_x0, dorm_sill, dorm_mid, dorm_peak, 2, Wood);
        fill_line(
            world,
            dorm_x1.saturating_sub(1),
            dorm_sill,
            dorm_mid,
            dorm_peak,
            2,
            Wood,
        );
        fill_rect(
            world,
            dorm_x0 + 2,
            dorm_peak + 2,
            dorm_x1.saturating_sub(2),
            dorm_sill,
            Glass,
        );
    }

    // Porch at ground-floor deck, posts rooted to grade.
    let porch_w = (bay * 3 / 4).clamp(6, 16);
    let porch_x0 = left.saturating_sub(porch_w);
    fill_rect(
        world,
        porch_x0,
        y_ground,
        left + wall_t,
        y_ground + floor_t,
        Concrete,
    );
    fill_column(world, porch_x0, y_ground, grade, 2, Wood);
    fill_column(world, porch_x0 + porch_w / 2, y_ground, grade, 2, Wood);
    fill_line(
        world,
        porch_x0.saturating_sub(1),
        y_ground,
        left + wall_t,
        y_upper + 1,
        2,
        Wood,
    );

    // --- chimney -----------------------------------------------------------
    let chim_top = y_ridge.saturating_sub(3);
    fill_rect(world, chim_x0, chim_top, chim_x1, grade, Stone);
    if flue_x1 > flue_x0 {
        fill_rect(
            world,
            flue_x0,
            chim_top + 2,
            flue_x1,
            y_ground.saturating_sub(2),
            Empty,
        );
    }
    // Hearth on ground floor, facing into the house.
    let hearth_h = storey_clear.clamp(3, 6);
    let hearth_top = y_ground.saturating_sub(hearth_h);
    fill_rect(
        world,
        chim_x0.saturating_sub(3),
        hearth_top,
        chim_x1,
        y_ground + floor_t,
        Stone,
    );
    fill_rect(
        world,
        chim_x0.saturating_sub(2),
        hearth_top + 1,
        chim_x0 + 1,
        y_ground,
        Empty,
    );
    if chim_x0 > 1 {
        world.paint(chim_x0 - 1, y_ground.saturating_sub(1), Ember);
    }
    // Flashing at roof penetration.
    fill_rect(
        world,
        chim_x0.saturating_sub(1),
        y_plate.saturating_sub(2),
        (chim_x1 + 1).min(w),
        y_plate + floor_t,
        Stone,
    );

    // --- exterior openings (only inside wall thickness) --------------------
    let door_h_ext = (storey_clear * 2 / 3).clamp(4, 14);
    let door_top = y_ground.saturating_sub(door_h_ext);
    if door_top >= gnd_ceil {
        fill_rect(world, left, door_top, left + wall_t, y_ground, Empty);
    }
    // Threshold tied into porch + floor deck.
    fill_rect(
        world,
        left.saturating_sub(1),
        y_ground,
        left + wall_t + 2,
        y_ground + floor_t,
        Wood,
    );

    let win_h = (storey_clear / 3).clamp(2, 7);
    let sill = 2usize;
    // Left wall windows — both living storeys, glass strictly inside wall_t.
    let g1 = y_ground.saturating_sub(sill);
    let g0 = g1.saturating_sub(win_h);
    if g0 >= gnd_ceil && g1 > g0 {
        fill_rect(world, left, g0, left + wall_t, g1, Glass);
    }
    let u1 = y_upper.saturating_sub(sill);
    let u0 = u1.saturating_sub(win_h);
    if u0 >= up_ceil && u1 > u0 {
        fill_rect(world, left, u0, left + wall_t, u1, Glass);
    }

    // --- furniture (rests on sky-side of each deck) ------------------------
    // Living sofa, left of stairs, sitting on ground deck.
    let sofa_x0 = left + wall_t + 2;
    let sofa_x1 = posts[stair_bay.min(posts.len() - 1)].saturating_sub(2);
    if sofa_x1 > sofa_x0 + 3 {
        fill_rect(
            world,
            sofa_x0,
            y_ground.saturating_sub(2),
            sofa_x1,
            y_ground,
            Wood,
        );
        fill_rect(
            world,
            sofa_x0,
            y_ground.saturating_sub(3),
            sofa_x0 + 2,
            y_ground.saturating_sub(2),
            Wood,
        );
        fill_rect(
            world,
            sofa_x1.saturating_sub(2),
            y_ground.saturating_sub(3),
            sofa_x1,
            y_ground.saturating_sub(2),
            Wood,
        );
    }

    // Kitchen block right of stairs, clear of chimney.
    let kit_x0 = stair_x1 + 2;
    let kit_x1 = chim_x0.saturating_sub(3);
    if kit_x1 > kit_x0 + 3 {
        fill_rect(
            world,
            kit_x0,
            y_ground.saturating_sub(2),
            kit_x1,
            y_ground,
            Wood,
        );
        fill_rect(
            world,
            kit_x0,
            y_ground.saturating_sub(4),
            kit_x0 + 3,
            y_ground.saturating_sub(2),
            Wood,
        );
    }

    // Beds on upper floor.
    if posts.len() > stair_bay {
        let bed_l1 = posts[stair_bay].saturating_sub(1);
        if bed_l1 > left + wall_t + 3 {
            fill_rect(
                world,
                left + wall_t + 2,
                y_upper.saturating_sub(2),
                bed_l1,
                y_upper,
                Wood,
            );
        }
    }
    let bed_r0 = stair_x1 + 2;
    let bed_r1 = chim_x0.saturating_sub(3);
    if bed_r1 > bed_r0 + 3 {
        fill_rect(
            world,
            bed_r0,
            y_upper.saturating_sub(2),
            bed_r1,
            y_upper,
            Wood,
        );
        fill_column(
            world,
            bed_r1.saturating_sub(1),
            y_upper.saturating_sub(4),
            y_upper,
            2,
            Wood,
        );
    }

    // Attic crates.
    if attic_clear >= 3 {
        fill_rect(
            world,
            left + wall_t + 2,
            y_attic.saturating_sub(2),
            left + wall_t + 2 + (bay / 2).max(4),
            y_attic,
            Wood,
        );
        if posts.len() > 2 {
            let cx0 = posts[posts.len() - 2] + 2;
            fill_rect(
                world,
                cx0,
                y_attic.saturating_sub(2),
                (cx0 + (bay / 2).max(4)).min(chim_x0.saturating_sub(2)),
                y_attic,
                Wood,
            );
        }
    }

    // --- basement services -------------------------------------------------
    let tank_x0 = left + wall_t + 2;
    let tank_x1 = posts
        .get(1)
        .copied()
        .unwrap_or(left + bay)
        .saturating_sub(2);
    let tank_top = bas_ceil + 1;
    let tank_bot = grade - 1;
    if tank_x1 > tank_x0 + 4 && tank_bot > tank_top + 3 {
        fill_rect(world, tank_x0, tank_top, tank_x1, tank_top + 1, Metal);
        fill_column(world, tank_x0, tank_top, tank_bot, 1, Metal);
        fill_column(world, tank_x1 - 1, tank_top, tank_bot, 1, Metal);
        fill_rect(world, tank_x0, tank_bot - 1, tank_x1, tank_bot, Metal);
        // Legs to slab.
        fill_column(world, tank_x0, tank_bot, grade, 1, Metal);
        fill_column(world, tank_x1 - 1, tank_bot, grade, 1, Metal);
        fill_rect(
            world,
            tank_x0 + 1,
            tank_top + 1,
            tank_x1 - 1,
            tank_bot - 1,
            Water,
        );
    }

    let fuel_x0 = stair_x1 + 2;
    let fuel_x1 = chim_x0.saturating_sub(2);
    if fuel_x1 > fuel_x0 + 3 {
        fill_rect(
            world,
            fuel_x0,
            grade.saturating_sub(3),
            fuel_x0 + 4,
            grade - 1,
            Coal,
        );
        fill_rect(
            world,
            fuel_x0 + 5,
            grade.saturating_sub(4),
            (fuel_x0 + 9).min(fuel_x1),
            grade - 1,
            Oil,
        );
        fill_rect(
            world,
            chim_x0.saturating_sub(1),
            grade.saturating_sub(3),
            chim_x1,
            grade - 1,
            Metal,
        );
    }

    // --- yard --------------------------------------------------------------
    if porch_x0 > 3 {
        fill_rect(world, 1, grade - 1, porch_x0, grade, Sand);
    }
    if left > 8 {
        plant_tree(
            world,
            2.max(left / 6),
            grade,
            (usable * 28 / 100).clamp(6, 28),
            (usable * 12 / 100).clamp(3, 12),
        );
        if left > 16 {
            plant_tree(
                world,
                left / 2,
                grade,
                (usable * 20 / 100).clamp(5, 20),
                (usable * 9 / 100).clamp(3, 9),
            );
        }
    }
    if w - right > 8 {
        plant_tree(
            world,
            right + 2,
            grade,
            (usable * 32 / 100).clamp(7, 30),
            (usable * 13 / 100).clamp(3, 13),
        );
        if w - right > 18 {
            plant_tree(
                world,
                w.saturating_sub(7),
                grade,
                (usable * 18 / 100).clamp(5, 18),
                (usable * 8 / 100).clamp(3, 8),
            );
        }
    }
    for sx in (foot_left..left).step_by(2) {
        world.paint(sx, grade - 1, Plant);
    }
    for sx in (right..foot_right.min(w)).step_by(2) {
        world.paint(sx, grade - 1, Plant);
    }

    // Rain barrel rooted on grade.
    let barrel_x = (right + 2).min(w.saturating_sub(5));
    if barrel_x + 3 < w && grade > 5 {
        fill_rect(world, barrel_x, grade - 4, barrel_x + 3, grade - 3, Metal);
        fill_column(world, barrel_x, grade - 4, grade, 1, Metal);
        fill_column(world, barrel_x + 2, grade - 4, grade, 1, Metal);
        fill_rect(world, barrel_x, grade - 1, barrel_x + 3, grade, Metal);
        fill_rect(
            world,
            barrel_x + 1,
            grade - 3,
            barrel_x + 2,
            grade - 1,
            Water,
        );
    }
}

/// A reinforced high-rise cut through offices, elevator core, services, and roof tank.
pub(super) fn seed_skyscraper(world: &mut World) {
    let (w, h) = (world.width, world.height);
    if w < 48 || h < 30 {
        return;
    }

    let ground = h - 3;
    let left = w / 5;
    let right = w * 4 / 5;
    let top = 5;
    let core_left = w / 2 - 3;
    let core_right = w / 2 + 3;
    let floor_height = ((ground - top) / 7).max(3);

    fill_rect(world, 0, ground, w, h, Stone);
    fill_rect(world, left - 2, ground - 5, right + 2, ground, Concrete);
    fill_rect(world, left, top, left + 2, ground, Concrete);
    fill_rect(world, right - 2, top, right, ground, Concrete);
    fill_rect(world, core_left, top, core_left + 2, ground, Concrete);
    fill_rect(world, core_right - 2, top, core_right, ground, Concrete);
    fill_rect(
        world,
        core_left + 2,
        top + 2,
        core_right - 2,
        ground - 1,
        Empty,
    );

    for floor in (top + floor_height..ground).step_by(floor_height) {
        fill_rect(world, left, floor, right, floor + 1, Concrete);
        fill_rect(world, left + 3, floor - 1, core_left - 2, floor, Wood);
        fill_rect(world, core_right + 2, floor - 1, right - 3, floor, Wood);
        fill_rect(
            world,
            left + 5,
            floor - floor_height + 2,
            left + 6,
            floor,
            Wood,
        );
        fill_rect(
            world,
            right - 6,
            floor - floor_height + 2,
            right - 5,
            floor,
            Wood,
        );
        fill_rect(
            world,
            left + 2,
            floor - floor_height + 3,
            left + 3,
            floor - 2,
            Glass,
        );
        fill_rect(
            world,
            right - 3,
            floor - floor_height + 3,
            right - 2,
            floor - 2,
            Glass,
        );
    }

    fill_rect(
        world,
        core_left + 2,
        ground - floor_height - 2,
        core_right - 2,
        ground - floor_height + 2,
        Metal,
    );
    fill_rect(world, right - 7, top + 2, right - 6, ground - 2, Metal);
    fill_rect(world, right - 6, top + 2, right - 5, ground - 2, Water);
    fill_rect(world, left + 4, ground - 4, core_left - 2, ground - 2, Oil);

    let tank_left = core_left - 5;
    let tank_right = core_right + 5;
    fill_rect(world, tank_left, 0, tank_right, 1, Metal);
    fill_rect(world, tank_left, 0, tank_left + 1, top + 2, Metal);
    fill_rect(world, tank_right - 1, 0, tank_right, top + 2, Metal);
    fill_rect(world, tank_left + 1, 1, tank_right - 1, top + 1, Water);
    fill_rect(world, tank_left, top + 1, tank_right, top + 2, Metal);
}
