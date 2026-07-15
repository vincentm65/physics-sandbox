use rand::Rng;

use crate::material::{AMBIENT_TEMP, Material};
use Material::*;

mod core;
mod effects;
mod heat;
mod reactions;
mod scenes;

pub use core::{Scene, World};

const FIRE_LIFE_MIN: u16 = 60;
const FIRE_LIFE_MAX: u16 = 100;
const EMBER_LIFE_MIN: u16 = 80;
const EMBER_LIFE_MAX: u16 = 170;
const STEAM_LIFE_MIN: u16 = 120;
const STEAM_LIFE_MAX: u16 = 280;
const SMOKE_LIFE_MIN: u16 = 80;
const SMOKE_LIFE_MAX: u16 = 180;
const GUNPOWDER_BLAST_RADIUS: i32 = 5;
const TNT_BLAST_RADIUS: i32 = 10;
const C4_BLAST_RADIUS: i32 = 12;
/// Ticks a fuse cell smoulders before flaring to fire and kindling its
/// neighbours. Sets the burn-front pace: one cell advances per this many ticks.
pub(crate) const FUSE_BURN_TICKS: u16 = 3;

const CHUNK_W: usize = 64;
const CHUNK_H: usize = 32;
const MAX_VELOCITY: i8 = 4;
const VELOCITY_SCALE: i8 = 4;
const GRAVITY_PER_TICK: i8 = 1;

/// Fraction of cooled embers that leave a residue of ash; the rest are fully
/// consumed by the burn.
const ASH_CHANCE_PER_MILLE: u32 = 50;

/// How far a liquid tries to flow sideways in one tick. Bigger = flatter water;
/// lava stays viscous.
fn spread_of(m: Material) -> usize {
    match m {
        Water | LiquidNitrogen => 6,
        Acid | Oil => 4,
        _ => 1,
    }
}

fn chunks_x(width: usize) -> usize {
    width.div_ceil(CHUNK_W)
}

fn chunks_y(height: usize) -> usize {
    height.div_ceil(CHUNK_H)
}

fn chunks_len(width: usize, height: usize) -> usize {
    chunks_x(width) * chunks_y(height)
}

fn activate_chunk_neighborhood(
    width: usize,
    height: usize,
    chunks_x: usize,
    x: usize,
    y: usize,
    chunks: &mut [bool],
) {
    if width == 0 || height == 0 || chunks_x == 0 || chunks.is_empty() {
        return;
    }

    let x0 = x.saturating_sub(1) / CHUNK_W;
    let y0 = y.saturating_sub(1) / CHUNK_H;
    let x1 = (x + 1).min(width - 1) / CHUNK_W;
    let y1 = (y + 1).min(height - 1) / CHUNK_H;

    for cy in y0..=y1 {
        for cx in x0..=x1 {
            if let Some(chunk) = chunks.get_mut(cy * chunks_x + cx) {
                *chunk = true;
            }
        }
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
