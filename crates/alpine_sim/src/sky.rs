//! The calendar, the sun, the moon and the light they give (decisions 33 to 35).
//!
//! Positions use low-precision formulas for the sun and moon (P. Schlyter, "How to compute planetary
//! positions", after J. Meeus), good to a minute or two for rise and set times. They were checked
//! against an independent table for Telve, 55 km west, for 11 March 1916 (docs/decisions.md).
//! Everything is plain `f64` arithmetic from the update count, so it repeats exactly.

use crate::tuning::LightTuning;

/// The sun's centre is this far below a flat horizon at sunrise and sunset (refraction and the
/// sun's half-width).
pub const SUNRISE_ALTITUDE_DEG: f64 = -0.833;
/// The same for the moon, measured from the observer, not the earth's centre.
pub const MOONRISE_ALTITUDE_DEG: f64 = -0.833;
/// The sun is this far below the horizon at the end of evening civil twilight ("dusk").
pub const DUSK_ALTITUDE_DEG: f64 = -6.0;
/// ... and at the start of morning civil twilight: first light, "dawn", the daily orders moment
/// (decision 37).
pub const DAWN_ALTITUDE_DEG: f64 = -6.0;
/// Fading of light by the air, in magnitudes per thickness of air straight up (a clear mountain sky).
const EXTINCTION_MAG: f64 = 0.2;

/// A date and time on the game's clock (local time, Central European Time in 1916).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// Seconds since local midnight.
    pub seconds: f64,
}

const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/// Days from 1 January 1970 to a date (proleptic Gregorian; H. Hinnant's algorithm).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The date `days` after 1 January 1970.
pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (yoe + era * 400 + if m <= 2 { 1 } else { 0 }) as i32;
    (y, m, d)
}

impl DateTime {
    /// Parses "YYYY-MM-DD HH:MM" (local time).
    pub fn parse(text: &str) -> Result<DateTime, String> {
        let bad = || format!("'{text}' is not a date and time like 1916-03-06 06:42");
        let (date, time) = text.trim().split_once(' ').ok_or_else(bad)?;
        let mut d = date.split('-').map(|p| p.parse::<i64>());
        let (y, m, day) = match (d.next(), d.next(), d.next(), d.next()) {
            (Some(Ok(y)), Some(Ok(m)), Some(Ok(day)), None) => (y, m, day),
            _ => return Err(bad()),
        };
        let (h, min) = time.trim().split_once(':').ok_or_else(bad)?;
        let (h, min): (u32, u32) = (h.parse().map_err(|_| bad())?, min.parse().map_err(|_| bad())?);
        if !(1..=12).contains(&m) || !(1..=31).contains(&day) || h > 23 || min > 59 {
            return Err(bad());
        }
        Ok(DateTime { year: y as i32, month: m as u32, day: day as u32, seconds: (h * 3600 + min * 60) as f64 })
    }

    /// Local seconds since 1 January 1970 00:00.
    pub fn to_seconds(self) -> f64 {
        days_from_civil(self.year, self.month, self.day) as f64 * 86400.0 + self.seconds
    }

    pub fn from_seconds(s: f64) -> DateTime {
        let days = (s / 86400.0).floor();
        let (year, month, day) = civil_from_days(days as i64);
        DateTime { year, month, day, seconds: s - days * 86400.0 }
    }

    /// "06:42".
    pub fn hhmm(self) -> String {
        let m = (self.seconds / 60.0).floor() as u32;
        format!("{:02}:{:02}", m / 60, m % 60)
    }

    /// "Monday 6 March 1916".
    pub fn long_date(self) -> String {
        let wd = (days_from_civil(self.year, self.month, self.day) + 3).rem_euclid(7) as usize; // 1 Jan 1970 was a Thursday
        format!("{} {} {} {}", WEEKDAYS[wd], self.day, MONTHS[self.month as usize - 1], self.year)
    }

    /// "Mon 6 Mar 1916".
    pub fn short_date(self) -> String {
        let wd = (days_from_civil(self.year, self.month, self.day) + 3).rem_euclid(7) as usize;
        format!("{} {} {} {}", &WEEKDAYS[wd][..3], self.day, &MONTHS[self.month as usize - 1][..3], self.year)
    }
}

/// Where and when: the observer's place and the clock's starting moment.
#[derive(Clone, Copy, Debug)]
pub struct Calendar {
    pub latitude: f64,
    pub longitude: f64,
    /// Local time minus universal time, in hours (1 for Central European Time).
    pub utc_offset_h: f64,
    /// Local seconds since 1970 at update 0.
    pub start: f64,
}

/// What the sky is doing at one moment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkyState {
    pub sun_altitude_deg: f64,
    pub moon_altitude_deg: f64,
    /// Share of the moon's disc that is lit, 0 to 1.
    pub moon_lit: f64,
    /// Whether the moon is growing (between new and full).
    pub moon_waxing: bool,
    /// Cloud cover, 0 (clear) to 1 (overcast).
    pub cloud: f64,
    /// Light on the ground, lux.
    pub lux: f64,
}

fn rev(x: f64) -> f64 {
    x.rem_euclid(360.0)
}
fn sind(x: f64) -> f64 {
    x.to_radians().sin()
}
fn cosd(x: f64) -> f64 {
    x.to_radians().cos()
}

/// Schlyter's day number: days since 2000 January 0.0 universal time.
fn day_number(local_seconds: f64, utc_offset_h: f64) -> f64 {
    let ut = local_seconds - utc_offset_h * 3600.0;
    ut / 86400.0 - days_from_civil(1999, 12, 31) as f64
}

/// The sun's ecliptic longitude, mean anomaly and argument of perihelion (degrees).
fn sun_ecliptic(d: f64) -> (f64, f64, f64) {
    let w = 282.9404 + 4.70935e-5 * d;
    let e = 0.016709 - 1.151e-9 * d;
    let m = rev(356.0470 + 0.985_600_258_5 * d);
    let ea = m + e.to_degrees() * sind(m) * (1.0 + e * cosd(m));
    let xv = cosd(ea) - e;
    let yv = (1.0 - e * e).sqrt() * sind(ea);
    let v = yv.atan2(xv).to_degrees();
    (rev(v + w), m, w)
}

/// Altitude above the horizon (degrees, from the earth's centre) of a body at the given ecliptic
/// longitude and latitude, seen from `cal`'s place at day number `d`.
fn altitude(cal: &Calendar, d: f64, lon: f64, lat: f64) -> f64 {
    let ecl = 23.4393 - 3.563e-7 * d;
    let (x, y, z) = (cosd(lon) * cosd(lat), sind(lon) * cosd(lat), sind(lat));
    let ye = y * cosd(ecl) - z * sind(ecl);
    let ze = y * sind(ecl) + z * cosd(ecl);
    let ra = ye.atan2(x).to_degrees();
    let dec = ze.atan2(x.hypot(ye)).to_degrees();
    let (_, m, w) = sun_ecliptic(d);
    let gmst0 = rev(m + w + 180.0);
    let ut_h = (d - d.floor()) * 24.0;
    let lst = gmst0 + ut_h * 15.04107 + cal.longitude;
    let ha = rev(lst - ra);
    let s = sind(cal.latitude) * sind(dec) + cosd(cal.latitude) * cosd(dec) * cosd(ha);
    s.clamp(-1.0, 1.0).asin().to_degrees()
}

/// The moon's ecliptic longitude and latitude (degrees) and lit share, with the main perturbations.
fn moon_ecliptic(d: f64) -> (f64, f64, f64, bool) {
    let n = rev(125.1228 - 0.052_953_808_3 * d);
    let i = 5.1454;
    let w = rev(318.0634 + 0.164_357_322_3 * d);
    let a = 60.2666;
    let e: f64 = 0.054900;
    let m = rev(115.3654 + 13.064_992_950_9 * d);
    let mut ea = m + e.to_degrees() * sind(m) * (1.0 + e * cosd(m));
    for _ in 0..5 {
        ea -= (ea - e.to_degrees() * sind(ea) - m) / (1.0 - e * cosd(ea));
    }
    let xv = a * (cosd(ea) - e);
    let yv = a * (1.0 - e * e).sqrt() * sind(ea);
    let v = yv.atan2(xv).to_degrees();
    let r = xv.hypot(yv);
    let xh = r * (cosd(n) * cosd(v + w) - sind(n) * sind(v + w) * cosd(i));
    let yh = r * (sind(n) * cosd(v + w) + cosd(n) * sind(v + w) * cosd(i));
    let zh = r * sind(v + w) * sind(i);
    let mut lon = rev(yh.atan2(xh).to_degrees());
    let mut lat = zh.atan2(xh.hypot(yh)).to_degrees();
    let (ls, ms, _) = sun_ecliptic(d);
    let lm = rev(n + w + m);
    let dd = rev(lm - ls);
    let f = rev(lm - n);
    lon += -1.274 * sind(m - 2.0 * dd) + 0.658 * sind(2.0 * dd) - 0.186 * sind(ms) - 0.059 * sind(2.0 * m - 2.0 * dd)
        - 0.057 * sind(m - 2.0 * dd + ms)
        + 0.053 * sind(m + 2.0 * dd)
        + 0.046 * sind(2.0 * dd - ms)
        + 0.041 * sind(m - ms)
        - 0.035 * sind(dd)
        - 0.031 * sind(m + ms);
    lat += -0.173 * sind(f - 2.0 * dd) - 0.055 * sind(m - f - 2.0 * dd) - 0.046 * sind(m + f - 2.0 * dd)
        + 0.033 * sind(f + 2.0 * dd)
        + 0.017 * sind(2.0 * m + f);
    let elongation = (cosd(lon - ls) * cosd(lat)).clamp(-1.0, 1.0).acos().to_degrees();
    let lit = (1.0 - cosd(elongation)) / 2.0;
    let waxing = rev(lon - ls) < 180.0;
    (lon, lat, lit, waxing)
}

/// Thickness of air towards a body at this altitude, relative to straight up (Kasten and Young).
fn air_mass(alt_deg: f64) -> f64 {
    let a = alt_deg.max(0.0);
    1.0 / (sind(a) + 0.50572 * (a + 6.07995).powf(-1.6364))
}

/// Interpolates `log10(y)` linearly against `x` over points sorted by `x`, clamping at the ends
/// of the table except that the low end keeps falling at the last slope.
fn log_interp_falling(points: &[[f64; 2]], x: f64) -> f64 {
    debug_assert!(points.len() >= 2);
    // Points are sorted by x descending (sun altitude from high to low).
    if x >= points[0][0] {
        return points[0][1];
    }
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        if x >= b[0] {
            let t = (x - a[0]) / (b[0] - a[0]);
            return 10f64.powf(a[1].log10() + t * (b[1].log10() - a[1].log10()));
        }
    }
    let (a, b) = (points[points.len() - 2], points[points.len() - 1]);
    let t = (x - a[0]) / (b[0] - a[0]);
    10f64.powf(a[1].log10() + t * (b[1].log10() - a[1].log10()))
}

impl Calendar {
    pub fn local_seconds(&self, game_seconds: f64) -> f64 {
        self.start + game_seconds
    }

    pub fn now(&self, game_seconds: f64) -> DateTime {
        DateTime::from_seconds(self.local_seconds(game_seconds))
    }

    pub fn sun_altitude(&self, local_seconds: f64) -> f64 {
        let d = day_number(local_seconds, self.utc_offset_h);
        let (lon, _, _) = sun_ecliptic(d);
        altitude(self, d, lon, 0.0)
    }

    /// The moon's altitude seen from the ground (corrected for its nearness), lit share, and
    /// whether it is growing.
    pub fn moon(&self, local_seconds: f64) -> (f64, f64, bool) {
        let d = day_number(local_seconds, self.utc_offset_h);
        let (lon, lat, lit, waxing) = moon_ecliptic(d);
        let geo = altitude(self, d, lon, lat);
        (geo - 0.9507 * cosd(geo), lit, waxing)
    }

    /// The first moment at or after `from` (local seconds) when `f` rises through `level`, searched
    /// minute by minute for up to `max_hours`, then refined to a tenth of a second.
    pub fn next_rise(&self, from: f64, max_hours: f64, level: f64, f: impl Fn(f64) -> f64) -> Option<f64> {
        self.next_crossing(from, max_hours, level, true, f)
    }

    pub fn next_set(&self, from: f64, max_hours: f64, level: f64, f: impl Fn(f64) -> f64) -> Option<f64> {
        self.next_crossing(from, max_hours, level, false, f)
    }

    fn next_crossing(&self, from: f64, max_hours: f64, level: f64, rising: bool, f: impl Fn(f64) -> f64) -> Option<f64> {
        // "Past" means on the far side of the crossing being looked for.
        let past = |t: f64| (f(t) >= level) == rising;
        let end = from + max_hours * 3600.0;
        let mut t = from;
        // Already past it at the start: wait until back on the near side.
        while t < end && past(t) {
            t += 60.0;
        }
        let above = past;
        while t < end {
            let next = t + 60.0;
            if above(next) {
                let (mut lo, mut hi) = (t, next);
                while hi - lo > 0.1 {
                    let mid = (lo + hi) / 2.0;
                    if above(mid) { hi = mid } else { lo = mid }
                }
                return Some(hi);
            }
            t = next;
        }
        None
    }

    /// Sunrise on the day containing `local_seconds` (local seconds since 1970).
    pub fn sunrise_on(&self, local_seconds: f64) -> Option<f64> {
        let midnight = (local_seconds / 86400.0).floor() * 86400.0;
        self.next_rise(midnight, 24.0, SUNRISE_ALTITUDE_DEG, |t| self.sun_altitude(t))
    }

    /// First light (dawn) on the day containing `local_seconds`.
    pub fn first_light_on(&self, local_seconds: f64) -> Option<f64> {
        let midnight = (local_seconds / 86400.0).floor() * 86400.0;
        self.next_rise(midnight, 24.0, DAWN_ALTITUDE_DEG, |t| self.sun_altitude(t))
    }

    pub fn sunset_on(&self, local_seconds: f64) -> Option<f64> {
        let midnight = (local_seconds / 86400.0).floor() * 86400.0;
        self.next_set(midnight + 12.0 * 3600.0, 12.0, SUNRISE_ALTITUDE_DEG, |t| self.sun_altitude(t))
    }

    /// The sky and the light at a moment, with the given cloud cover.
    pub fn sky(&self, local_seconds: f64, cloud: f64, light: &LightTuning) -> SkyState {
        let sun = self.sun_altitude(local_seconds);
        let (moon_alt, moon_lit, moon_waxing) = self.moon(local_seconds);
        let sun_lux = log_interp_falling(&light.sun_lux_by_altitude, sun);
        let sun_lux = if sun > 0.0 { sun_lux.max(light.sun_lux_by_altitude[0][1] + 120_000.0 * sind(sun)) } else { sun_lux };
        // The moon: Allen's phase law for its brightness, times the height in the sky, dimmed by the
        // thickness of air.
        let moon_lux = if moon_alt > 0.0 {
            let phase_angle = (1.0 - 2.0 * moon_lit).clamp(-1.0, 1.0).acos().to_degrees();
            let phase = 10f64.powf(-0.4 * (0.026 * phase_angle + 4.0e-9 * phase_angle.powi(4)));
            light.full_moon_lux * phase * sind(moon_alt) * 10f64.powf(-0.4 * EXTINCTION_MAG * (air_mass(moon_alt) - 1.0))
        } else {
            0.0
        };
        let clear = sun_lux + moon_lux + light.starlight_lux;
        let lux = clear * light.overcast_light_factor.powf(cloud.clamp(0.0, 1.0));
        SkyState { sun_altitude_deg: sun, moon_altitude_deg: moon_alt, moon_lit, moon_waxing, cloud, lux }
    }
}

/// How far a man can be spotted in this light: the tuning table read as a straight line between
/// its points on log-log paper, clamped at both ends and never beyond `max_m`.
pub fn spotting_distance(lux: f64, light: &LightTuning, max_m: f32) -> f32 {
    let mut pts: Vec<[f64; 2]> = light.spotting_by_light.clone();
    pts.sort_by(|a, b| b[0].total_cmp(&a[0]));
    let d = if lux >= pts[0][0] {
        pts[0][1]
    } else if lux <= pts[pts.len() - 1][0] {
        pts[pts.len() - 1][1]
    } else {
        let mut out = pts[pts.len() - 1][1];
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if lux >= b[0] {
                let t = (lux.log10() - a[0].log10()) / (b[0].log10() - a[0].log10());
                out = 10f64.powf(a[1].log10() + t * (b[1].log10() - a[1].log10()));
                break;
            }
        }
        out
    };
    (d as f32).min(max_m)
}

/// A plain description of the moon, e.g. "a thin crescent (9% lit), growing".
pub fn moon_words(lit: f64, waxing: bool) -> String {
    let shape = match lit {
        l if l < 0.03 => "new",
        l if l < 0.35 => "a crescent",
        l if l < 0.65 => "about half",
        l if l < 0.97 => "gibbous",
        _ => "full",
    };
    format!("{shape} ({:.0}% lit), {}", lit * 100.0, if waxing { "growing" } else { "waning" })
}

/// "clear", "partly cloudy" or "overcast".
pub fn cloud_words(cloud: f64) -> &'static str {
    match cloud {
        c if c < 0.25 => "clear",
        c if c < 0.75 => "partly cloudy",
        _ => "overcast",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col_di_lana(start: &str) -> Calendar {
        Calendar { latitude: 46.50, longitude: 11.96, utc_offset_h: 1.0, start: DateTime::parse(start).unwrap().to_seconds() }
    }

    #[test]
    fn dates_round_trip() {
        let t = DateTime::parse("1916-03-06 06:42").unwrap();
        assert_eq!(t.long_date(), "Monday 6 March 1916");
        assert_eq!(DateTime::from_seconds(t.to_seconds() + 6.0 * 86400.0).long_date(), "Sunday 12 March 1916");
        assert_eq!(t.hhmm(), "06:42");
    }

    #[test]
    fn sunrise_matches_decision_31() {
        let c = col_di_lana("1916-03-06 00:00");
        // Decision 31 gives the times to the nearest minute.
        for (day, minutes) in [(0.0, 6 * 60 + 42), (6.0, 6 * 60 + 31)] {
            let rise = DateTime::from_seconds(c.sunrise_on(c.start + day * 86400.0).unwrap());
            assert!((rise.seconds / 60.0 - minutes as f64).abs() <= 1.0, "{rise:?}");
        }
    }

    #[test]
    fn spotting_table_hits_its_points() {
        let l = LightTuning::default();
        for p in &l.spotting_by_light {
            let d = spotting_distance(p[0], &l, 1500.0);
            assert!((d as f64 - p[1]).abs() < 0.01, "{p:?} -> {d}");
        }
        assert_eq!(spotting_distance(1.0e-9, &l, 1500.0), 15.0);
        assert_eq!(spotting_distance(1.0e6, &l, 1500.0), 1500.0);
    }
}
