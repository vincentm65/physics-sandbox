use ratatui::style::Color;
use serde::{Deserialize, Serialize};

/// Every substance in the sandbox.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Material {
    Empty,
    Stone,
    Wood,
    Sand,
    Water,
    Oil,
    Acid,
    Lava,
    Fire,
    Steam,
    Ember,
    Ash,
    Smoke,
    Salt,
    Ice,
    Gunpowder,
    Plant,
    Mercury,
    Tnt,
    Fuse,
    C4,
    Napalm,
    Coal,
    Glass,
    Metal,
    LiquidNitrogen,
    Faucet,
    Drain,
    Concrete,
    Firework,
    FireworkSpark,
    BrokenGlass,
}
use Material::*;

/// Approximate ambient air temperature in Celsius.
pub const AMBIENT_TEMP: i16 = 20;

impl Material {
    /// `[key, material]` pairs for the on-screen palette.
    pub const PALETTE: [(char, Material); 10] = [
        ('1', Salt),
        ('2', Water),
        ('3', Stone),
        ('4', Wood),
        ('5', Fire),
        ('6', Lava),
        ('7', Oil),
        ('8', Acid),
        ('9', Mercury),
        ('0', Empty),
    ];

    /// Every material, grouped by phase, in the order the picker lists them.
    pub const ALL: [Material; 31] = [
        // fireworks
        Firework,
        // powders / granular
        Sand,
        BrokenGlass,
        Ash,
        Salt,
        Gunpowder,
        Coal,
        // liquids
        Water,
        Oil,
        Napalm,
        Acid,
        Lava,
        LiquidNitrogen,
        Mercury,
        // solids and explosives
        Stone,
        Concrete,
        Metal,
        Glass,
        Wood,
        Ice,
        Plant,
        Fuse,
        Tnt,
        C4,
        // fire, gases, and tools
        Fire,
        Ember,
        Steam,
        Smoke,
        // tools
        Faucet,
        Drain,
        Empty,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Empty => "Eraser",
            Stone => "Stone",
            Wood => "Wood",
            Sand => "Sand",
            Water => "Water",
            Oil => "Oil",
            Acid => "Acid",
            Lava => "Lava",
            Fire => "Fire",
            Steam => "Steam",
            Ember => "Ember",
            Ash => "Ash",
            Smoke => "Smoke",
            Salt => "Salt",
            Ice => "Ice",
            Gunpowder => "Gunpowder",
            Plant => "Plant",
            Mercury => "Mercury",
            Tnt => "TNT",
            Fuse => "Fuse",
            C4 => "C4",
            Napalm => "Napalm",
            Coal => "Coal",
            Glass => "Glass",
            BrokenGlass => "Broken glass",
            Metal => "Metal",

            LiquidNitrogen => "Liquid nitrogen",
            Concrete => "Concrete",
            Firework => "Firework",
            FireworkSpark => "Firework spark",
            Faucet => "Faucet",
            Drain => "Drain",
        }
    }

    /// Relative heaviness. Higher sinks below lower. Statics are immovable.
    pub fn density(self) -> i32 {
        match self {
            Smoke => 1,
            Steam => 1,
            Oil => 2,
            Water => 3,
            Ice => 2,
            Ash => 4,
            Salt => 6,
            Sand => 5,
            Wood => 5,
            Plant => 5,
            Ember => 6,
            Lava => 6,
            Stone => 7,
            Gunpowder => 8,
            Mercury => 12,
            Napalm => 2,
            LiquidNitrogen => 1,
            Coal => 6,
            Glass => 7,
            BrokenGlass => 6,
            Concrete => 10,

            Metal => 14,
            FireworkSpark => 1,
            Faucet | Drain | Firework => 0,
            _ => 0,
        }
    }

    pub fn is_liquid(self) -> bool {
        matches!(
            self,
            Water | Oil | Napalm | Acid | Lava | LiquidNitrogen | Mercury
        )
    }
    pub fn is_empty(self) -> bool {
        matches!(self, Empty)
    }
    pub fn is_gas(self) -> bool {
        matches!(self, Fire | Steam | Smoke | FireworkSpark)
    }

    /// Anything gravity/buoyancy can displace by swapping cells.
    pub fn is_fluid(self) -> bool {
        self.is_gas()
            || self.is_liquid()
            || matches!(
                self,
                Sand | BrokenGlass | Ash | Ember | Salt | Gunpowder | Coal
            )
    }
    pub fn can_sink_into(self, other: Material) -> bool {
        other.is_empty() || (other.is_fluid() && self.density() > other.density())
    }

    /// Oils and gels that float and resist ordinary water extinguishing.
    pub fn is_oily(self) -> bool {
        matches!(self, Oil | Napalm)
    }

    /// Combustible fuels that ignite into fire/ember. Structural solids use
    /// [`melt`] instead of burning.
    pub fn flammable(self) -> bool {
        self.combustion().is_some()
    }

    /// `(minimum temperature °C, ignition delay ticks, burn lifetime)`.
    pub fn combustion(self) -> Option<(u16, u16, u16)> {
        match self {
            Plant => Some((230, 24, 100)),
            Napalm => Some((250, 24, 300)),
            Wood => Some((300, 48, 160)),
            Oil => Some((350, 24, 120)),
            Coal => Some((500, 64, 550)),
            _ => None,
        }
    }

    /// Product left when a fuel finishes its ignition transition.
    pub fn burn_product(self) -> Material {
        match self {
            Wood | Plant | Coal => Ember,
            _ => Fire,
        }
    }

    /// Heat-driven phase change: `(melt temperature °C, soak delay, product)`.
    /// Structural materials crack or melt instead of becoming flame.
    pub fn melt(self) -> Option<(u16, u16, Material)> {
        match self {
            Ice => Some((1, 4, Water)),
            Glass => Some((1_100, 90, BrokenGlass)),
            Stone => Some((1_200, 150, Sand)),
            Concrete => Some((1_250, 180, Sand)),
            Metal => Some((1_250, 220, Lava)),
            Sand | BrokenGlass => Some((1_280, 200, Lava)),
            _ => None,
        }
    }

    /// Fixed temperature this material forces when present (heat/cold source).
    /// `None` means the cell free-floats thermally.
    pub fn heat_source_temp(self) -> Option<i16> {
        match self {
            Lava => Some(1_300),
            Fire => Some(900),
            Ember => Some(700),
            Smoke => Some(60),
            LiquidNitrogen => Some(-196),
            Ice => Some(-5),
            _ => None,
        }
    }

    /// Temperature assigned when the material is painted into the world.
    pub fn painted_temperature(self) -> i16 {
        match self {
            Steam => 105,
            _ => self.heat_source_temp().unwrap_or(AMBIENT_TEMP),
        }
    }

    /// How quickly this material equalizes with neighbors (0 = insulator, 8 = metal).
    pub fn thermal_conductivity(self) -> i16 {
        match self {
            Metal => 8,
            Empty | Fire | Steam | Smoke => 6,
            Water | Acid | Lava | LiquidNitrogen | Mercury | Oil | Napalm => 5,
            Glass | BrokenGlass | Ice | Sand | Salt | Ash => 4,
            Ember | Gunpowder | Coal => 3,
            Wood | Plant | Fuse | Tnt | C4 | Firework | Stone | Concrete => 2,
            FireworkSpark => 6,
            Faucet | Drain => 3,
        }
    }

    /// Per-mille chance acid fails to etch this material (1000 = immune).
    pub fn acid_resistance(self) -> u32 {
        match self {
            Empty | Acid => 1000,
            Stone | Concrete | Glass | BrokenGlass => 1000,
            Metal => 920,
            Sand | Ash | Salt | Coal | Gunpowder => 100,
            Ice => 50,
            Plant | Wood | Fuse | Firework => 0,
            Oil | Napalm | Water | Mercury | Lava | LiquidNitrogen => 700,
            Fire | Ember | Steam | Smoke | FireworkSpark | Tnt | C4 | Faucet | Drain => 1000,
        }
    }

    /// Hit points before a blast breaks this solid. `None` means the material is
    /// not damage-tracked (fluids, powders, and soft cells use other rules).
    pub fn blast_hp(self) -> Option<u8> {
        match self {
            Glass => Some(8),
            Wood | Plant | Ice => Some(12),
            Stone => Some(16),
            Concrete => Some(40),
            Metal => Some(56),
            _ => None,
        }
    }

    /// Debris left when accumulated blast damage exceeds [`blast_hp`].
    pub fn blast_break_product(self) -> Option<Material> {
        match self {
            Glass => Some(BrokenGlass),
            Stone | Concrete => Some(Sand),
            Wood | Plant => Some(Ash),
            Ice => Some(Water),
            Metal => Some(Empty),
            _ => None,
        }
    }

    /// Napalm clings to solids instead of flowing freely.
    pub fn sticky(self) -> bool {
        matches!(self, Napalm)
    }

    /// Stable on-disk encoding. Keep this independent from `ALL`, which is UI order.
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Reconstruct a material from its stable on-disk encoding.
    pub fn from_u8(v: u8) -> Option<Self> {
        // Discriminants follow the enum declaration order.
        match v {
            0 => Some(Empty),
            1 => Some(Stone),
            2 => Some(Wood),
            3 => Some(Sand),
            4 => Some(Water),
            5 => Some(Oil),
            6 => Some(Acid),
            7 => Some(Lava),
            8 => Some(Fire),
            9 => Some(Steam),
            10 => Some(Ember),
            11 => Some(Ash),
            12 => Some(Smoke),
            13 => Some(Salt),
            14 => Some(Ice),
            15 => Some(Gunpowder),
            16 => Some(Plant),
            17 => Some(Mercury),
            18 => Some(Tnt),
            19 => Some(Fuse),
            20 => Some(C4),
            21 => Some(Napalm),
            22 => Some(Coal),
            23 => Some(Glass),
            24 => Some(Metal),
            25 => Some(LiquidNitrogen),
            26 => Some(Faucet),
            27 => Some(Drain),
            28 => Some(Concrete),
            29 => Some(Firework),
            30 => Some(FireworkSpark),
            31 => Some(BrokenGlass),
            _ => None,
        }
    }

    /// Per-cell colour. `seed` rides along with a grain for stable texture;
    /// `tick` only animates fire/lava so the rest does not flicker.
    pub fn color(self, seed: u8, life: u16, tick: u64) -> Color {
        let ts = (seed as u32).wrapping_mul(2_654_435_761) >> 16;
        let tt = ts.wrapping_add((tick as u32).wrapping_mul(40_503));
        let rs = |a: u32, n: u32| (a + (ts % n)) as u8;
        let rt = |a: u32, n: u32| (a + (tt % n)) as u8;

        match self {
            Empty => Color::Rgb(8, 10, 16),
            Stone => Color::Rgb(rs(95, 30), rs(95, 30), rs(105, 30)),
            Wood => Color::Rgb(128, 80, 45),
            Sand => Color::Rgb(rs(206, 40), rs(184, 40), rs(96, 26)),
            Water => Color::Rgb(rs(38, 18), rs(104, 30), rs(226, 24)),
            Oil => Color::Rgb(rs(48, 16), rs(36, 12), rs(22, 10)),
            Acid => Color::Rgb(rs(120, 40), 235, rs(58, 40)),
            Lava => Color::Rgb(rt(210, 45), rt(58, 90), rt(18, 18)),
            Fire => {
                if life > 50 {
                    Color::Rgb(rt(250, 6), rt(232, 12), rt(150, 30))
                } else if life > 30 {
                    Color::Rgb(rt(244, 12), rt(148, 30), rt(38, 20))
                } else {
                    Color::Rgb(rt(208, 20), rt(54, 20), rt(20, 14))
                }
            }
            Steam => {
                let b = 130 + (life / 4).min(110) as u8;
                Color::Rgb(b, b, b.wrapping_add(12))
            }
            // glowing coal: deep pulsing orange-red, dimmer than fire/lava
            Ember => Color::Rgb(rt(200, 45), rt(74, 50), rt(22, 18)),
            Ash => Color::Rgb(rs(98, 16), rs(94, 16), rs(90, 16)),
            Smoke => {
                let g = 28 + (life / 4).min(70) as u8;
                Color::Rgb(g, g, g.wrapping_add(6))
            }
            Salt => Color::Rgb(rs(235, 12), rs(230, 12), rs(220, 12)),
            // A narrow shared shift keeps every crystal in the same cool-blue family.
            Ice => Color::Rgb(rs(200, 10), rs(222, 10), rs(246, 10)),
            Gunpowder => Color::Rgb(rs(42, 12), rs(40, 12), rs(38, 12)),
            Plant => Color::Rgb(rs(30, 18), rs(148, 30), rs(30, 18)),
            Mercury => Color::Rgb(rs(168, 18), rs(172, 18), rs(180, 18)),
            Tnt => Color::Rgb(rs(205, 35), rs(24, 18), rs(18, 12)),
            Fuse => {
                if life > 0 {
                    // Burning fuse tip: hot pulsing glow as the front passes.
                    Color::Rgb(rt(240, 15), rt(150, 40), rt(40, 20))
                } else {
                    Color::Rgb(rs(110, 24), rs(82, 20), rs(42, 16))
                }
            }
            C4 => Color::Rgb(rs(76, 24), rs(92, 24), rs(38, 14)),
            Napalm => Color::Rgb(rt(220, 30), rt(72, 35), rt(18, 16)),
            Coal => Color::Rgb(rs(24, 16), rs(24, 16), rs(28, 18)),
            // Intact pane: cool cyan-teal, tight variance so it reads as one colour.
            Glass => Color::Rgb(rs(148, 14), rs(200, 12), rs(214, 10)),
            // Shards: same family, slightly paler/frosted with more grain variance.
            BrokenGlass => Color::Rgb(rs(168, 32), rs(204, 24), rs(216, 22)),
            Metal => Color::Rgb(rs(150, 35), rs(158, 35), rs(168, 35)),
            LiquidNitrogen => Color::Rgb(rs(180, 25), rs(225, 25), 250),
            Faucet => Color::Rgb(rs(140, 30), rs(148, 30), rs(155, 30)),
            Drain => Color::Rgb(rs(48, 18), rs(52, 18), rs(58, 18)),
            Concrete => Color::Rgb(rs(100, 20), rs(100, 20), rs(108, 20)),
            Firework => {
                if life == 0 {
                    Color::Rgb(rs(88, 24), rs(42, 16), rs(26, 12))
                } else {
                    Color::Rgb(rt(240, 16), rt(196, 40), rt(80, 30))
                }
            }
            FireworkSpark => {
                // Twinkle brightly at first, then dim rather than disappearing abruptly.
                let glow = (life.min(18) as u8).saturating_mul(13).saturating_add(
                    if (tick + seed as u64).is_multiple_of(3) {
                        20
                    } else {
                        0
                    },
                );
                match seed % 6 {
                    0 => Color::Rgb(glow, glow / 4, glow / 6),
                    1 => Color::Rgb(glow / 5, glow, glow / 3),
                    2 => Color::Rgb(glow / 5, glow / 2, glow),
                    3 => Color::Rgb(glow, glow.saturating_mul(3) / 4, glow / 8),
                    4 => Color::Rgb(glow, glow / 4, glow.saturating_mul(2) / 3),
                    _ => Color::Rgb(glow.saturating_mul(3) / 4, glow / 2, glow),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Material::{Ice, Wood};
    use ratatui::style::Color;

    #[test]
    fn wood_is_brown_and_ice_variations_stay_in_their_color_family() {
        for seed in 0..=u8::MAX {
            assert_eq!(Wood.color(seed, 0, 0), Color::Rgb(128, 80, 45));

            assert!(matches!(
                Ice.color(seed, 0, 0),
                Color::Rgb(200..=209, 222..=231, 246..=255)
            ));
        }
    }
}
