//! The weather diary (decision 34): cloud cover through the week, read from a data file.
//! Between two entries the cover changes evenly; before the first and after the last it holds.

use crate::sky::DateTime;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct DiaryFile {
    cloud: Vec<CloudEntry>,
}

#[derive(Debug, Deserialize)]
struct CloudEntry {
    at: String,
    cover: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Weather {
    /// (local seconds since 1970, cloud cover 0 to 1), in time order.
    cloud: Vec<(f64, f64)>,
}

impl Weather {
    /// A sky with the same cloud cover all the time (for tests).
    pub fn constant(cover: f64) -> Weather {
        Weather { cloud: vec![(0.0, cover.clamp(0.0, 1.0))] }
    }

    pub fn from_toml(text: &str) -> Result<Weather, String> {
        let file: DiaryFile = toml::from_str(text).map_err(|e| format!("weather diary: {e}"))?;
        let mut cloud = Vec::new();
        for e in &file.cloud {
            let t = DateTime::parse(&e.at).map_err(|m| format!("weather diary: {m}"))?.to_seconds();
            if !(0.0..=1.0).contains(&e.cover) {
                return Err(format!("weather diary: cloud cover at {} must be between 0 and 1", e.at));
            }
            if cloud.last().is_some_and(|&(p, _)| t <= p) {
                return Err(format!("weather diary: entry {} is not later than the one before it", e.at));
            }
            cloud.push((t, e.cover));
        }
        if cloud.is_empty() {
            return Err("weather diary: no cloud entries".into());
        }
        Ok(Weather { cloud })
    }

    /// The week of the step-2 game, `data/weather/march_1916.toml`, and its text.
    pub fn load_march_1916() -> Result<(Weather, String), String> {
        let path = crate::data_dir().join("weather").join("march_1916.toml");
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok((Weather::from_toml(&text)?, text))
    }

    /// Cloud cover at a moment (local seconds since 1970).
    pub fn cloud_at(&self, t: f64) -> f64 {
        let c = &self.cloud;
        if t <= c[0].0 {
            return c[0].1;
        }
        for w in c.windows(2) {
            let ((t0, a), (t1, b)) = (w[0], w[1]);
            if t <= t1 {
                return a + (b - a) * (t - t0) / (t1 - t0);
            }
        }
        c[c.len() - 1].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_and_holds() {
        let w = Weather::from_toml(
            "cloud = [ { at = \"1916-03-06 00:00\", cover = 0.0 }, { at = \"1916-03-06 02:00\", cover = 1.0 } ]",
        )
        .unwrap();
        let t0 = DateTime::parse("1916-03-06 00:00").unwrap().to_seconds();
        assert_eq!(w.cloud_at(t0 - 10.0), 0.0);
        assert!((w.cloud_at(t0 + 3600.0) - 0.5).abs() < 1e-9);
        assert_eq!(w.cloud_at(t0 + 99999.0), 1.0);
    }

    #[test]
    fn the_diary_loads() {
        Weather::load_march_1916().unwrap();
    }
}
