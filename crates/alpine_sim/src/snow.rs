//! Falling snow (decisions 57 to 67): fresh snow lying on each grid point of the map, which builds up
//! while it falls (the weather diary says when), settles afterwards and slows the men who walk
//! through it; walking packs it down into tracks, and routes prefer them. Snowshoes slow a man less.
//! Falling snow shortens how far men see, and the dawn report forecasts it, sometimes wrongly.
//! Read from `data/snow.toml`. Without that file (the locked tests 1 to 37) none of this happens.
//!
//! Every part can be switched off on its own in the file (decision 65); switched off, the game
//! behaves as if that part had never been built.

use serde::Deserialize;
use std::sync::Arc;

#[derive(Clone, Debug, Deserialize)]
pub struct SnowFile {
    #[serde(default)]
    pub switches: SnowSwitches,
    pub snowshoes: SnowshoeTuning,
    pub walking: SnowWalking,
    pub packing: PackingTuning,
    pub settling: SettlingTuning,
    pub sight: SnowSight,
    pub forecast: ForecastTuning,
}

fn yes() -> bool {
    true
}

/// Each part of falling snow, on or off (decision 65).
#[derive(Clone, Debug, Deserialize)]
pub struct SnowSwitches {
    /// Snow falls as the weather diary says. Off: no snow ever falls, and the whole snow piece is off.
    #[serde(default = "yes")]
    pub snowfall: bool,
    /// Fresh snow slows the men walking through it.
    #[serde(default = "yes")]
    pub slows_walking: bool,
    /// Walking packs fresh snow down into tracks. Off: fresh snow slows everyone alike, nobody packs it.
    #[serde(default = "yes")]
    pub trail_breaking: bool,
    /// The snowshoe squads walk on snowshoes. Off: everyone goes on foot.
    #[serde(default = "yes")]
    pub snowshoes: bool,
    /// Routes count deep fresh snow as slow ground, so squads use packed tracks (decision 67).
    #[serde(default = "yes")]
    pub routes_use_tracks: bool,
    /// Falling snow shortens how far men see.
    #[serde(default = "yes")]
    pub cuts_sight: bool,
    /// The dawn report forecasts snow.
    #[serde(default = "yes")]
    pub forecast: bool,
    /// Some forecasts are wrong.
    #[serde(default = "yes")]
    pub wrong_forecasts: bool,
}

impl Default for SnowSwitches {
    fn default() -> Self {
        SnowSwitches {
            snowfall: true,
            slows_walking: true,
            trail_breaking: true,
            snowshoes: true,
            routes_use_tracks: true,
            cuts_sight: true,
            forecast: true,
            wrong_forecasts: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct SnowshoeTuning {
    /// The squads that have snowshoes, by their numbers in the game.
    pub squads: Vec<u32>,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SnowWalking {
    /// Pairs of [centimetres of fresh snow, share of his speed a man keeps], on foot and on snowshoes.
    pub on_foot: Vec<[f32; 2]>,
    pub snowshoes: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PackingTuning {
    /// This many men walking across a grid square leave this share of its fresh snow loose.
    pub men_to_pack: f32,
    pub left_loose: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SettlingTuning {
    /// Fresh snow settles to half its depth in this many hours.
    pub half_life_hours: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SnowSight {
    /// Pairs of [snowfall in cm an hour, metres a man is spotted at by day while it falls].
    pub by_snowfall: Vec<[f32; 2]>,
    /// Words for how hard it is snowing: light below the first rate, moderate below the second,
    /// heavy from it.
    pub moderate_from_cm_per_hour: f32,
    pub heavy_from_cm_per_hour: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ForecastTuning {
    /// A forecast says snow when at least this much falls in its half of the day, and heavy snow
    /// from the second figure.
    pub snow_from_cm: f32,
    pub heavy_from_cm: f32,
    /// The chance that a forecast for one half of the day is wrong.
    pub wrong_chance: f32,
}

impl SnowFile {
    pub fn from_toml(text: &str) -> Result<SnowFile, String> {
        let f: SnowFile = toml::from_str(text).map_err(|e| format!("snow file: {e}"))?;
        for (name, t) in [("walking.on_foot", &f.walking.on_foot), ("walking.snowshoes", &f.walking.snowshoes), ("sight.by_snowfall", &f.sight.by_snowfall)]
        {
            if t.is_empty() || t.windows(2).any(|w| w[1][0] <= w[0][0]) {
                return Err(format!("snow file: {name} needs points in rising order"));
            }
        }
        if !(0.0..1.0).contains(&f.packing.left_loose) || f.packing.left_loose <= 0.0 || f.packing.men_to_pack <= 0.0 {
            return Err("snow file: packing needs men_to_pack above 0 and left_loose between 0 and 1".into());
        }
        Ok(f)
    }

    /// The share of his speed a man keeps in this much fresh snow.
    pub fn walking_factor(&self, cm: f32, snowshoes: bool) -> f32 {
        if !self.switches.slows_walking {
            return 1.0;
        }
        let table = if snowshoes && self.switches.snowshoes { &self.walking.snowshoes } else { &self.walking.on_foot };
        table_at(table, cm).clamp(0.05, 1.0)
    }

    /// How far a man is spotted by day while snow falls at this rate, as a share of `clear_m`.
    pub fn sight_factor(&self, cm_per_hour: f32, clear_m: f32) -> f32 {
        if !self.switches.cuts_sight || cm_per_hour <= 0.0 {
            return 1.0;
        }
        (table_at(&self.sight.by_snowfall, cm_per_hour) / clear_m.max(1.0)).clamp(0.0, 1.0)
    }

    /// "light", "moderate" or "heavy" (snow falling at this rate); "" when none falls.
    pub fn falling_word(&self, cm_per_hour: f32) -> &'static str {
        if cm_per_hour < 0.05 {
            ""
        } else if cm_per_hour < self.sight.moderate_from_cm_per_hour {
            "light"
        } else if cm_per_hour < self.sight.heavy_from_cm_per_hour {
            "moderate"
        } else {
            "heavy"
        }
    }

    /// How a forecast describes this much snow in half a day.
    pub fn amount(&self, cm: f32) -> SnowAmount {
        if cm < self.forecast.snow_from_cm {
            SnowAmount::None
        } else if cm < self.forecast.heavy_from_cm {
            SnowAmount::Light
        } else {
            SnowAmount::Heavy
        }
    }

    /// Packing per metre walked: walking `d` metres across a square leaves exp(-k d) of its loose snow.
    pub fn packing_per_m(&self, spacing: f32) -> f32 {
        (1.0 / self.packing.left_loose).ln() / (self.packing.men_to_pack * spacing.max(0.1))
    }
}

/// A table of [x, y] points read with straight lines between them, holding at the ends.
pub fn table_at(t: &[[f32; 2]], x: f32) -> f32 {
    if x <= t[0][0] {
        return t[0][1];
    }
    for w in t.windows(2) {
        let ([x0, y0], [x1, y1]) = (w[0], w[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    t[t.len() - 1][1]
}

/// How much snow a forecast speaks of for half a day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnowAmount {
    None,
    Light,
    Heavy,
}

impl SnowAmount {
    pub fn index(self) -> u32 {
        self as u32
    }
}

/// The forecast given at a dawn, for the day (to 18:00) and the night after (to 06:00).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forecast {
    /// What the forecast says.
    pub said: [SnowAmount; 2],
    /// What the weather diary will bring (for checking; the player is not told).
    pub truth: [SnowAmount; 2],
}

impl Forecast {
    /// "Forecast: no snow today; heavy snow likely tonight."
    pub fn words(&self) -> String {
        let part = |a: SnowAmount, when: &str| match a {
            SnowAmount::None => format!("no snow {when}"),
            SnowAmount::Light => format!("light snow likely {when}"),
            SnowAmount::Heavy => format!("heavy snow likely {when}"),
        };
        format!("Forecast: {}; {}.", part(self.said[0], "today"), part(self.said[1], "tonight"))
    }
}

/// Fresh snow on the map and everything else falling snow keeps track of.
#[derive(Clone, Debug)]
pub struct Snow {
    pub tuning: SnowFile,
    /// Centimetres of loose fresh snow at each grid point (what slows a man).
    pub loose: Vec<f32>,
    /// Centimetres of fresh snow packed down by walking at each grid point (the tracks).
    pub packed: Vec<f32>,
    /// The loose snow as it was at the latest game minute, for working out routes on worker
    /// threads (decision 67); refreshed once a game minute.
    pub for_routes: Arc<Vec<f32>>,
    /// Snow falling now, in cm an hour.
    pub falling: f32,
    /// Centimetres of loose fresh snow on ground nobody has walked on.
    pub untrodden: f32,
    /// Centimetres fallen since the latest dawn, and in the 24 hours before it.
    pub fallen_today: f32,
    pub fallen_yesterday: Option<f32>,
    /// The forecast given at the latest dawn (with the forecast switched on).
    pub forecast: Option<Forecast>,
}

impl Snow {
    pub fn new(tuning: SnowFile, points: usize) -> Snow {
        Snow {
            tuning,
            loose: vec![0.0; points],
            packed: vec![0.0; points],
            for_routes: Arc::new(vec![0.0; points]),
            falling: 0.0,
            untrodden: 0.0,
            fallen_today: 0.0,
            fallen_yesterday: None,
            forecast: None,
        }
    }

    /// Whether routes take the snow into account now.
    pub fn routes_see_snow(&self) -> bool {
        self.tuning.switches.routes_use_tracks && self.tuning.switches.slows_walking
    }

    /// How strongly a grid point shows as a track, 0 to 1: packed snow with little loose snow on it.
    pub fn track(&self, i: usize) -> f32 {
        let (p, l) = (self.packed[i], self.loose[i]);
        if p < 1.0 {
            return 0.0;
        }
        (p / (p + l)).clamp(0.0, 1.0) * ((p - 1.0) / 8.0).clamp(0.0, 1.0)
    }
}

/// What a route needs to know about the snow: the loose snow when it was planned, and the walking
/// table of the squad's footwear.
#[derive(Clone, Debug)]
pub struct RouteSnow {
    pub loose: Arc<Vec<f32>>,
    pub table: Vec<[f32; 2]>,
}

impl RouteSnow {
    /// The share of his speed a man keeps at grid point `i`.
    #[inline]
    pub fn factor(&self, i: usize) -> f32 {
        table_at(&self.table, self.loose[i]).clamp(0.05, 1.0)
    }
}

/// `data/snow.toml`'s text.
pub fn load_text() -> Result<String, String> {
    let path = crate::data_dir().join("snow.toml");
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snow_file_loads() {
        let f = SnowFile::from_toml(&load_text().unwrap()).unwrap();
        assert!(f.walking_factor(40.0, true) > f.walking_factor(40.0, false));
        assert_eq!(f.walking_factor(0.0, false), 1.0);
    }

    #[test]
    fn tables_hold_at_the_ends() {
        let t = [[0.0, 1.0], [10.0, 0.5]];
        assert_eq!(table_at(&t, -1.0), 1.0);
        assert_eq!(table_at(&t, 5.0), 0.75);
        assert_eq!(table_at(&t, 99.0), 0.5);
    }
}
