use ratatui::style::Color;

/// Every substance in the sandbox.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Material {
    Empty,
    Wall,
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
}

use Material::*;

impl Material {
    /// `[key, material]` pairs for the on-screen palette.
    pub const PALETTE: [(char, Material); 10] = [
        ('1', Sand),
        ('2', Water),
        ('3', Wall),
        ('4', Wood),
        ('5', Fire),
        ('6', Lava),
        ('7', Oil),
        ('8', Acid),
        ('9', Steam),
        ('0', Empty),
    ];

    /// Every material, grouped by phase, in the order the picker lists them.
    /// (Ember/Ash/Smoke have no number key, so the picker is the only way to
    ///  reach them.)
    pub const ALL: [Material; 13] = [
        // powders / granular
        Sand,
        Ash,
        // liquids
        Water,
        Oil,
        Acid,
        Lava,
        // solids
        Wall,
        Wood,
        // fire & gases
        Fire,
        Ember,
        Smoke,
        Steam,
        // tools
        Empty,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Empty => "Eraser",
            Wall => "Stone",
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
        }
    }

    /// Relative heaviness. Higher sinks below lower. Statics are immovable.
    pub fn density(self) -> i32 {
        match self {
            Smoke => 1,
            Steam => 1,
            Oil => 2,
            Water => 3,
            Acid => 4,
            Ash => 4,
            Sand => 5,
            Ember => 6,
            Lava => 6,
            _ => 0,
        }
    }

    pub fn is_liquid(self) -> bool {
        matches!(self, Water | Oil | Acid | Lava)
    }
    pub fn is_empty(self) -> bool {
        matches!(self, Empty)
    }
    /// Anything gravity/buoyancy can displace by swapping cells.
    pub fn is_fluid(self) -> bool {
        matches!(self, Water | Oil | Acid | Lava | Sand | Steam | Ash | Ember | Smoke)
    }
    pub fn flammable(self) -> bool {
        matches!(self, Wood | Oil)
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
            Wall => Color::Rgb(rs(95, 30), rs(95, 30), rs(105, 30)),
            Wood => Color::Rgb(rs(108, 26), rs(66, 18), rs(34, 12)),
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
        }
    }
}
