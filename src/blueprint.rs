use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::material::Material;
use crate::raster;
use crate::scene_manager::SceneState;
use crate::world::World;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blueprint {
    pub name: String,
    pub width: usize,
    pub height: usize,
    #[serde(default)]
    pub operations: Vec<Operation>,
    #[serde(default)]
    pub prefabs: HashMap<String, Vec<Operation>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Rect {
        x: i32,
        y: i32,
        width: usize,
        height: usize,
        material: Material,
    },
    HollowRect {
        x: i32,
        y: i32,
        width: usize,
        height: usize,
        #[serde(default = "one")]
        thickness: usize,
        material: Material,
    },
    Line {
        from: [i32; 2],
        to: [i32; 2],
        #[serde(default = "one")]
        thickness: usize,
        material: Material,
    },
    Disc {
        center: [i32; 2],
        radius: usize,
        material: Material,
    },
    Ground {
        #[serde(default = "one")]
        thickness: usize,
        material: Material,
    },
    Building {
        x: i32,
        y: i32,
        width: usize,
        height: usize,
        #[serde(default = "one")]
        wall_thickness: usize,
        #[serde(default = "one")]
        floors: usize,
        material: Material,
    },
    Stairs {
        x: i32,
        y: i32,
        steps: usize,
        #[serde(default = "one")]
        step_width: usize,
        #[serde(default)]
        rise_left: bool,
        material: Material,
    },
    Tank {
        x: i32,
        y: i32,
        width: usize,
        height: usize,
        #[serde(default = "one")]
        thickness: usize,
        wall: Material,
        contents: Material,
        #[serde(default = "default_fill")]
        fill: f32,
    },
    Repeat {
        count: usize,
        offset: [i32; 2],
        operations: Vec<Operation>,
    },
    Prefab {
        name: String,
        x: i32,
        y: i32,
    },
    Terrain {
        points: Vec<[i32; 2]>,
        depth: usize,
        material: Material,
    },
    Vary {
        x: i32,
        y: i32,
        width: usize,
        height: usize,
        from: Material,
        materials: Vec<Material>,
        chance: f64,
        seed: u64,
    },
}

fn one() -> usize {
    1
}
fn default_fill() -> f32 {
    0.75
}

pub fn load(path: &Path) -> Result<Blueprint, String> {
    let data =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&data).map_err(|e| format!("invalid blueprint: {e}"))
}

impl Blueprint {
    pub fn validate(&self) -> Result<Vec<String>, String> {
        if self.name.trim().is_empty() {
            return Err("name must not be empty".into());
        }
        if self.width == 0 || self.height == 0 {
            return Err("width and height must be greater than zero".into());
        }
        if self.width.checked_mul(self.height).is_none() {
            return Err("map dimensions are too large".into());
        }
        // Prefab coordinates are local; validate their fields but discard bounds warnings.
        for (name, ops) in &self.prefabs {
            validate_operations(
                ops,
                self.width,
                self.height,
                &format!("prefab({name})"),
                &mut Vec::new(),
                &self.prefabs,
                &mut HashSet::new(),
            )?;
        }
        let mut warnings = Vec::new();
        let mut expanding = HashSet::new();
        validate_operations(
            &self.operations,
            self.width,
            self.height,
            "operations",
            &mut warnings,
            &self.prefabs,
            &mut expanding,
        )?;
        Ok(warnings)
    }

    pub fn build(&self) -> Result<World, String> {
        self.validate()?;
        let mut world = World::new(self.width, self.height);
        apply_operations(&mut world, &self.operations, 0, 0, &self.prefabs);
        Ok(world)
    }

    pub fn scene_state(&self) -> Result<SceneState, String> {
        Ok(SceneState::from_world(&self.build()?, self.name.clone()))
    }
}

fn validate_operations(
    ops: &[Operation],
    w: usize,
    h: usize,
    path: &str,
    warnings: &mut Vec<String>,
    prefabs: &HashMap<String, Vec<Operation>>,
    expanding: &mut HashSet<String>,
) -> Result<(), String> {
    for (i, op) in ops.iter().enumerate() {
        let at = format!("{path}[{i}]");
        match op {
            Operation::Rect {
                x,
                y,
                width,
                height,
                ..
            }
            | Operation::HollowRect {
                x,
                y,
                width,
                height,
                ..
            }
            | Operation::Building {
                x,
                y,
                width,
                height,
                ..
            }
            | Operation::Tank {
                x,
                y,
                width,
                height,
                ..
            } => {
                nonzero(*width, *height, &at)?;
                warn_bounds(*x, *y, *width, *height, [w, h], &at, warnings);
            }
            Operation::Line {
                from,
                to,
                thickness,
                ..
            } => {
                if *thickness == 0 {
                    return Err(format!("{at}: thickness must be greater than zero"));
                }
                let min_x = from[0].min(to[0]);
                let min_y = from[1].min(to[1]);
                let width = from[0].abs_diff(to[0]) as usize + *thickness;
                let height = from[1].abs_diff(to[1]) as usize + *thickness;
                warn_bounds(min_x, min_y, width, height, [w, h], &at, warnings);
            }
            Operation::Disc { center, radius, .. } => {
                if *radius == 0 {
                    return Err(format!("{at}: radius must be greater than zero"));
                }
                warn_bounds(
                    center[0] - *radius as i32,
                    center[1] - *radius as i32,
                    radius * 2 + 1,
                    radius * 2 + 1,
                    [w, h],
                    &at,
                    warnings,
                );
            }
            Operation::Ground { thickness, .. } => {
                if *thickness == 0 {
                    return Err(format!("{at}: thickness must be greater than zero"));
                }
            }
            Operation::Stairs {
                steps, step_width, ..
            } => {
                if *steps == 0 || *step_width == 0 {
                    return Err(format!(
                        "{at}: steps and step_width must be greater than zero"
                    ));
                }
            }
            Operation::Repeat {
                count, operations, ..
            } => {
                if *count == 0 {
                    return Err(format!("{at}: count must be greater than zero"));
                }
                validate_operations(
                    operations,
                    w,
                    h,
                    &format!("{at}.operations"),
                    warnings,
                    prefabs,
                    expanding,
                )?;
            }
            Operation::Prefab { name, .. } => {
                let sub = prefabs
                    .get(name)
                    .ok_or_else(|| format!("{at}: unknown prefab '{name}'"))?;
                if !expanding.insert(name.clone()) {
                    return Err(format!("{at}: cyclic prefab reference involving '{name}'"));
                }
                validate_operations(
                    sub,
                    w,
                    h,
                    &format!("{at}.prefab({name})"),
                    &mut Vec::new(),
                    prefabs,
                    expanding,
                )?;
                expanding.remove(name);
            }
            Operation::Terrain { points, depth, .. } => {
                if points.len() < 2 {
                    return Err(format!("{at}: terrain requires at least 2 points"));
                }
                if *depth == 0 {
                    return Err(format!("{at}: depth must be greater than zero"));
                }
                for (j, p) in points.iter().enumerate() {
                    warn_bounds(
                        p[0],
                        p[1],
                        1,
                        *depth,
                        [w, h],
                        &format!("{at}.points[{j}]"),
                        warnings,
                    );
                }
            }
            Operation::Vary {
                x,
                y,
                width,
                height,
                materials,
                chance,
                ..
            } => {
                if *width == 0 || *height == 0 {
                    return Err(format!("{at}: width and height must be greater than zero"));
                }
                if materials.is_empty() {
                    return Err(format!("{at}: materials must not be empty"));
                }
                if !(0.0..=1.0).contains(chance) {
                    return Err(format!("{at}: chance must be between 0 and 1"));
                }
                warn_bounds(*x, *y, *width, *height, [w, h], &at, warnings);
            }
        }
        match op {
            Operation::HollowRect {
                thickness,
                width,
                height,
                ..
            }
            | Operation::Building {
                wall_thickness: thickness,
                width,
                height,
                ..
            }
            | Operation::Tank {
                thickness,
                width,
                height,
                ..
            } if *thickness == 0 || thickness * 2 >= *width || thickness * 2 >= *height => {
                return Err(format!("{at}: thickness leaves no interior"));
            }
            Operation::Building { floors, .. } if *floors == 0 => {
                return Err(format!("{at}: floors must be greater than zero"));
            }
            Operation::Tank { fill, contents, .. } if !(0.0..=1.0).contains(fill) => {
                return Err(format!("{at}: fill must be between 0 and 1"));
            }
            Operation::Tank { contents, .. }
                if !contents.is_liquid()
                    && !matches!(
                        contents,
                        Material::Sand
                            | Material::BrokenGlass
                            | Material::Salt
                            | Material::Gunpowder
                            | Material::Coal
                    ) =>
            {
                warnings.push(format!("{at}: contents is not a liquid or powder"))
            }
            _ => {}
        }
    }
    Ok(())
}

fn nonzero(width: usize, height: usize, at: &str) -> Result<(), String> {
    if width == 0 || height == 0 {
        Err(format!("{at}: width and height must be greater than zero"))
    } else {
        Ok(())
    }
}

fn warn_bounds(
    x: i32,
    y: i32,
    width: usize,
    height: usize,
    [w, h]: [usize; 2],
    at: &str,
    warnings: &mut Vec<String>,
) {
    if x < 0 || y < 0 || x as i64 + width as i64 > w as i64 || y as i64 + height as i64 > h as i64 {
        warnings.push(format!(
            "{at}: shape extends outside the map and will be clipped"
        ));
    }
}

fn apply_operations(
    world: &mut World,
    ops: &[Operation],
    dx: i32,
    dy: i32,
    prefabs: &HashMap<String, Vec<Operation>>,
) {
    for op in ops {
        match op {
            Operation::Rect {
                x,
                y,
                width,
                height,
                material,
            } => rect(world, x + dx, y + dy, *width, *height, *material),
            Operation::HollowRect {
                x,
                y,
                width,
                height,
                thickness,
                material,
            } => hollow_rect(
                world,
                x + dx,
                y + dy,
                *width,
                *height,
                *thickness,
                *material,
            ),
            Operation::Line {
                from,
                to,
                thickness,
                material,
            } => line(
                world,
                [from[0] + dx, from[1] + dy],
                [to[0] + dx, to[1] + dy],
                *thickness,
                *material,
            ),
            Operation::Disc {
                center,
                radius,
                material,
            } => disc(world, [center[0] + dx, center[1] + dy], *radius, *material),
            Operation::Ground {
                thickness,
                material,
            } => rect(
                world,
                0,
                world.height as i32 - *thickness as i32,
                world.width,
                *thickness,
                *material,
            ),
            Operation::Building {
                x,
                y,
                width,
                height,
                wall_thickness,
                floors,
                material,
            } => building(
                world,
                [x + dx, y + dy],
                *width,
                *height,
                *wall_thickness,
                *floors,
                *material,
            ),
            Operation::Stairs {
                x,
                y,
                steps,
                step_width,
                rise_left,
                material,
            } => stairs(
                world,
                x + dx,
                y + dy,
                *steps,
                *step_width,
                *rise_left,
                *material,
            ),
            Operation::Tank {
                x,
                y,
                width,
                height,
                thickness,
                wall,
                contents,
                fill,
            } => tank(
                world,
                x + dx,
                y + dy,
                *width,
                *height,
                *thickness,
                *wall,
                *contents,
                *fill,
            ),
            Operation::Repeat {
                count,
                offset,
                operations,
            } => {
                for i in 0..*count {
                    apply_operations(
                        world,
                        operations,
                        dx + offset[0] * i as i32,
                        dy + offset[1] * i as i32,
                        prefabs,
                    );
                }
            }
            Operation::Prefab { name, x, y } => {
                if let Some(sub) = prefabs.get(name) {
                    apply_operations(world, sub, dx + x, dy + y, prefabs);
                }
            }
            Operation::Terrain {
                points,
                depth,
                material,
            } => terrain(world, points, *depth, *material),
            Operation::Vary {
                x,
                y,
                width,
                height,
                from,
                materials,
                chance,
                seed,
            } => vary(
                world,
                x + dx,
                y + dy,
                *width,
                *height,
                *from,
                materials,
                *chance,
                *seed,
            ),
        }
    }
}

fn paint(world: &mut World, x: i32, y: i32, material: Material) {
    if x >= 0 && y >= 0 {
        world.paint(x as usize, y as usize, material);
    }
}

pub(crate) fn rect(
    world: &mut World,
    x: i32,
    y: i32,
    width: usize,
    height: usize,
    material: Material,
) {
    for py in 0..height {
        for px in 0..width {
            paint(world, x + px as i32, y + py as i32, material);
        }
    }
}

pub(crate) fn hollow_rect(
    world: &mut World,
    x: i32,
    y: i32,
    width: usize,
    height: usize,
    thickness: usize,
    material: Material,
) {
    rect(world, x, y, width, thickness, material);
    rect(
        world,
        x,
        y + height as i32 - thickness as i32,
        width,
        thickness,
        material,
    );
    rect(world, x, y, thickness, height, material);
    rect(
        world,
        x + width as i32 - thickness as i32,
        y,
        thickness,
        height,
        material,
    );
}

pub(crate) fn line(
    world: &mut World,
    from: [i32; 2],
    to: [i32; 2],
    thickness: usize,
    material: Material,
) {
    for (x, y) in raster::line_points((from[0], from[1]), (to[0], to[1])) {
        rect(
            world,
            x - thickness as i32 / 2,
            y - thickness as i32 / 2,
            thickness,
            thickness,
            material,
        );
    }
}

pub(crate) fn disc(world: &mut World, center: [i32; 2], radius: usize, material: Material) {
    let r = radius as i32;
    for y in -r..=r {
        for x in -r..=r {
            if x * x + y * y <= r * r {
                paint(world, center[0] + x, center[1] + y, material);
            }
        }
    }
}

fn building(
    world: &mut World,
    [x, y]: [i32; 2],
    width: usize,
    height: usize,
    thickness: usize,
    floors: usize,
    material: Material,
) {
    hollow_rect(world, x, y, width, height, thickness, material);
    for floor in 1..floors {
        let fy = y + height as i32 - (height * floor / floors) as i32;
        rect(world, x, fy, width, thickness, material);
    }
}

fn stairs(
    world: &mut World,
    x: i32,
    y: i32,
    steps: usize,
    step_width: usize,
    rise_left: bool,
    material: Material,
) {
    for i in 0..steps {
        let px = if rise_left {
            x + ((steps - 1 - i) * step_width) as i32
        } else {
            x + (i * step_width) as i32
        };
        rect(world, px, y - i as i32, step_width, i + 1, material);
    }
}

#[allow(clippy::too_many_arguments)]
fn tank(
    world: &mut World,
    x: i32,
    y: i32,
    width: usize,
    height: usize,
    thickness: usize,
    wall: Material,
    contents: Material,
    fill: f32,
) {
    hollow_rect(world, x, y, width, height, thickness, wall);
    let inner_h = height - thickness * 2;
    let fill_h = (inner_h as f32 * fill).round() as usize;
    rect(
        world,
        x + thickness as i32,
        y + height as i32 - thickness as i32 - fill_h as i32,
        width - thickness * 2,
        fill_h,
        contents,
    );
}

/// Rasterize the polyline connecting `points` and fill each rasterized
/// cell downward by `depth` rows.
fn terrain(world: &mut World, points: &[[i32; 2]], depth: usize, material: Material) {
    let mut seen: Vec<[i32; 2]> = Vec::new();
    for i in 0..points.len().saturating_sub(1) {
        for (x, y) in raster::line_points(
            (points[i][0], points[i][1]),
            (points[i + 1][0], points[i + 1][1]),
        ) {
            let p = [x, y];
            if !seen.contains(&p) {
                seen.push(p);
            }
        }
    }
    for [px, py] in &seen {
        for d in 0..depth {
            paint(world, *px, *py + d as i32, material);
        }
    }
}

/// Replace cells equal to `from` within the rectangle using deterministic
/// coordinate+seed hashing.
#[allow(clippy::too_many_arguments)]
fn vary(
    world: &mut World,
    x: i32,
    y: i32,
    width: usize,
    height: usize,
    from: Material,
    materials: &[Material],
    chance: f64,
    seed: u64,
) {
    let threshold = (chance * 10_000.0) as u64;
    for dy in 0..height {
        for dx in 0..width {
            let cx = x + dx as i32;
            let cy = y + dy as i32;
            if cx < 0 || cy < 0 {
                continue;
            }
            let (ux, uy) = (cx as usize, cy as usize);
            if ux >= world.width || uy >= world.height {
                continue;
            }
            if world.get(ux, uy) != from {
                continue;
            }
            let h = vary_hash(cx, cy, seed);
            if h % 10_000 < threshold {
                let idx = (h as usize) % materials.len();
                world.paint(ux, uy, materials[idx]);
            }
        }
    }
}

/// Deterministic hash of (x, y, seed) using splitmix64-style mixing.
fn vary_hash(x: i32, y: i32, seed: u64) -> u64 {
    let mut state = seed;
    state = state
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(x as u64);
    state = state
        .wrapping_mul(0x85EB_CA6B_C1B7_4E2B)
        .wrapping_add(y as u64);
    state ^= state >> 33;
    state = state.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    state ^= state >> 33;
    state = state.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    state ^= state >> 33;
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_composed_blueprint() {
        let blueprint: Blueprint = serde_json::from_str(
            r#"{
                "name":"test", "width":30, "height":20,
                "operations":[
                    {"op":"ground", "thickness":2, "material":"stone"},
                    {"op":"building", "x":4, "y":5, "width":15, "height":13, "floors":2, "material":"wood"},
                    {"op":"tank", "x":21, "y":10, "width":7, "height":8, "wall":"stone", "contents":"water", "fill":0.5}
                ]
            }"#,
        )
        .unwrap();

        let world = blueprint.build().unwrap();
        assert_eq!(world.get(0, 19), Material::Stone);
        assert_eq!(world.get(4, 5), Material::Wood);
        assert_eq!(world.get(24, 15), Material::Water);
    }

    #[test]
    fn line_rasterizes_diagonal_endpoints() {
        let mut world = World::new(6, 6);
        line(&mut world, [1, 1], [4, 4], 1, Material::Stone);

        for point in 1..=4 {
            assert_eq!(world.get(point, point), Material::Stone);
        }
        assert_eq!(world.get(0, 0), Material::Empty);
        assert_eq!(world.get(5, 5), Material::Empty);
    }

    #[test]
    fn rejects_invalid_geometry() {
        let blueprint: Blueprint = serde_json::from_str(
            r#"{"name":"bad","width":10,"height":10,"operations":[
                {"op":"hollow_rect","x":1,"y":1,"width":2,"height":2,"thickness":1,"material":"stone"}
            ]}"#,
        )
        .unwrap();
        assert!(blueprint.validate().is_err());
    }

    // --- prefab tests ---

    #[test]
    fn prefab_applies_translated_operations() {
        let blueprint: Blueprint = serde_json::from_str(
            r#"{
                "name":"pf","width":50,"height":50,
                "prefabs":{
                    "dot": [{"op":"rect","x":0,"y":0,"width":1,"height":1,"material":"stone"}]
                },
                "operations":[
                    {"op":"prefab","name":"dot","x":10,"y":10}
                ]
            }"#,
        )
        .unwrap();
        let world = blueprint.build().unwrap();
        assert_eq!(world.get(10, 10), Material::Stone);
        assert_eq!(world.get(0, 0), Material::Empty);
    }

    #[test]
    fn prefab_unknown_name_rejected() {
        let blueprint: Blueprint = serde_json::from_str(
            r#"{
                "name":"pf","width":50,"height":50,
                "prefabs":{},
                "operations":[
                    {"op":"prefab","name":"nope","x":0,"y":0}
                ]
            }"#,
        )
        .unwrap();
        assert!(blueprint.validate().is_err());
    }

    #[test]
    fn prefab_cyclic_reference_rejected() {
        let blueprint: Blueprint = serde_json::from_str(
            r#"{
                "name":"pf","width":50,"height":50,
                "prefabs":{
                    "a": [{"op":"prefab","name":"b","x":0,"y":0}],
                    "b": [{"op":"prefab","name":"a","x":0,"y":0}]
                },
                "operations":[{"op":"prefab","name":"a","x":0,"y":0}]
            }"#,
        )
        .unwrap();
        assert!(blueprint.validate().is_err());
    }

    // --- terrain tests ---

    #[test]
    fn terrain_fills_downward_from_polyline() {
        let blueprint: Blueprint = serde_json::from_str(
            r#"{
                "name":"t","width":20,"height":20,
                "operations":[
                    {"op":"terrain","points":[[5,5],[5,8]],"depth":2,"material":"stone"}
                ]
            }"#,
        )
        .unwrap();
        let world = blueprint.build().unwrap();
        // points: (5,5)-(5,6)-(5,7)-(5,8) vertically.
        // Each fills 2 cells downward -> (5,5)-(5,6), (5,6)-(5,7), etc.
        // Combined: cells (5,5) through (5,9) should all be stone.
        for row in 5..=9 {
            assert_eq!(
                world.get(5, row),
                Material::Stone,
                "terrain fill at (5,{row})"
            );
        }
        // (5,10) should be empty
        assert_eq!(world.get(5, 10), Material::Empty);
    }

    #[test]
    fn terrain_requires_minimum_two_points() {
        let blueprint: Blueprint = serde_json::from_str(
            r#"{
                "name":"t","width":20,"height":20,
                "operations":[
                    {"op":"terrain","points":[[5,5]],"depth":3,"material":"stone"}
                ]
            }"#,
        )
        .unwrap();
        assert!(blueprint.validate().is_err());
    }

    // --- vary tests ---

    #[test]
    fn vary_replaces_deterministically() {
        let json = r#"{
            "name":"v","width":10,"height":10,
            "operations":[
                {"op":"rect","x":0,"y":0,"width":10,"height":10,"material":"stone"},
                {"op":"vary","x":0,"y":0,"width":5,"height":5,"from":"stone","materials":["sand","water"],"chance":1.0,"seed":42}
            ]
        }"#;
        let bp: Blueprint = serde_json::from_str(json).unwrap();
        let w1 = bp.build().unwrap();
        let w2 = bp.build().unwrap();
        for y in 0..5 {
            for x in 0..5 {
                assert_eq!(w1.get(x, y), w2.get(x, y), "mismatch at ({x},{y})");
            }
        }
    }

    #[test]
    fn vary_only_replaces_source_material() {
        let json = r#"{
            "name":"v","width":10,"height":10,
            "operations":[
                {"op":"rect","x":0,"y":0,"width":10,"height":10,"material":"stone"},
                {"op":"rect","x":3,"y":0,"width":1,"height":10,"material":"wood"},
                {"op":"vary","x":0,"y":0,"width":10,"height":10,"from":"stone","materials":["sand"],"chance":1.0,"seed":7}
            ]
        }"#;
        let bp: Blueprint = serde_json::from_str(json).unwrap();
        let world = bp.build().unwrap();
        assert_eq!(world.get(0, 0), Material::Sand);
        assert_eq!(world.get(2, 5), Material::Sand);
        for row in 0..10 {
            assert_eq!(
                world.get(3, row),
                Material::Wood,
                "wood at (3,{row}) was changed"
            );
        }
    }
}
