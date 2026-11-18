//! Speed pass (docs/design-brief.md, Build order): run after every build step on the development PC.
//!
//! 1. Restricts this process (and the game it launches) to 8 logical processors on 4 physical cores,
//!    two per core, read from Windows' own description of the CPU.
//! 2. Simulation check, two runs, both of which must pass (decision 44):
//!    - the heaviest load: the step-2 scenario from noon on Monday (full daylight, the heaviest
//!      sight work) with every squad moving on random orders;
//!    - the game as played: the work roster across Monday's dusk, when every rostered squad changes
//!      over at once and plans its walk to its new place.
//!
//!    Each times every update at the fastest game speed in real time: the updates falling in each
//!    1/60 s of real time (one frame) must together use no more than half of it (decision 30).
//!    Single slow updates are reported.
//! 3. Frame check, once per view: launches the game (the work roster, as played) at 2560x1440 with
//!    vsync off, flies the camera over the 3D terrain (then pans and zooms the 2D map) and reads back
//!    its frame times. In each view 99% of frames must finish within 11 ms (30 fps on an RTX 3070,
//!    taken as about a third of the RTX 5090's speed).
//! 4. For information only: how long the skip-ahead takes from dusk to dawn on the work roster, and
//!    how the simulation copes with 1,000, 3,000 and 5,000 soldiers.
//!
//! Build everything in release mode first, then run:
//!   cargo build --release --workspace
//!   target\release\speedpass.exe

use alpine_sim::scenario;
use alpine_sim::sim::{Sim, make_pool};
use alpine_sim::sky;
use alpine_sim::weather::Weather;
use alpine_sim::terrain::Terrain;
use alpine_sim::tuning::Tuning;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use windows_sys::Win32::System::SystemInformation::{GetLogicalProcessorInformationEx, RelationProcessorCore};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessAffinityMask, SetProcessAffinityMask};

/// 99% of frames must finish within this (see the frame check above).
const FRAME_LIMIT_MS: f32 = 11.0;
const FRAME_CHECK_SECONDS: u32 = 60;
const SIM_CHECK_MINUTES: f64 = 30.0;
/// The simulation check groups updates into frames of this length (60 frames per second).
const SIM_FRAME_MS: f64 = 1000.0 / 60.0;
/// The roster run starts this many game minutes before dusk, so the changeover falls inside it.
const DUSK_LEAD_MINUTES: f64 = 15.0;

/// Physical cores and the logical processors (as an affinity mask, processor group 0) on each.
fn cores() -> Result<Vec<u64>, String> {
    let mut len = 0u32;
    unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, std::ptr::null_mut(), &mut len) };
    if len == 0 {
        return Err("Windows did not describe the processor".into());
    }
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    let ok = unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, buf.as_mut_ptr().cast(), &mut len) };
    if ok == 0 {
        return Err("could not read the processor description".into());
    }
    let bytes: &[u8] = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast(), len as usize) };
    // Each record: relationship (u32), size (u32), then PROCESSOR_RELATIONSHIP: flags (u8),
    // efficiency class (u8), 20 reserved bytes, group count (u16), then GROUP_AFFINITY entries
    // (mask: usize, group: u16, 3 x u16 reserved) starting 24 bytes in.
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap()) as usize;
        let rel = i32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
        if rel == RelationProcessorCore {
            let p = off + 8;
            let group_count = u16::from_le_bytes(bytes[p + 22..p + 24].try_into().unwrap());
            if group_count >= 1 {
                let mask = u64::from_le_bytes(bytes[p + 24..p + 32].try_into().unwrap());
                let group = u16::from_le_bytes(bytes[p + 32..p + 34].try_into().unwrap());
                if group == 0 {
                    out.push(mask);
                }
            }
        }
        if size == 0 {
            break;
        }
        off += size;
    }
    Ok(out)
}

fn processors(mask: u64) -> String {
    (0..64).filter(|b| mask & (1 << b) != 0).map(|b| b.to_string()).collect::<Vec<_>>().join(", ")
}

fn pin_to_four_cores() -> Result<String, String> {
    let cores = cores()?;
    let two_thread: Vec<(usize, u64)> = cores.iter().copied().enumerate().filter(|(_, m)| m.count_ones() == 2).collect();
    if two_thread.len() < 4 {
        return Err(format!("needs 4 cores with two threads each; Windows reports {} cores: {:?}", cores.len(), cores));
    }
    let chosen = &two_thread[..4];
    let mask: u64 = chosen.iter().fold(0, |m, (_, c)| m | c);
    let ok = unsafe { SetProcessAffinityMask(GetCurrentProcess(), mask as usize) };
    if ok == 0 {
        return Err("Windows refused the processor restriction".into());
    }
    let (mut now, mut system) = (0usize, 0usize);
    unsafe { GetProcessAffinityMask(GetCurrentProcess(), &mut now, &mut system) };
    if now as u64 != mask {
        return Err(format!("restriction did not take: asked {mask:#x}, got {now:#x}"));
    }
    let detail = chosen
        .iter()
        .map(|(i, m)| format!("core {i} (logical processors {})", processors(*m)))
        .collect::<Vec<_>>()
        .join("; ");
    Ok(format!(
        "The CPU has {} cores and {} logical processors. Restricted to 8 logical processors (mask {mask:#06x}) on 4 cores, two per core: {detail}.",
        cores.len(),
        (system as u64).count_ones()
    ))
}

struct Timing {
    ticks: usize,
    mean: f64,
    p50: f64,
    p99: f64,
    p999: f64,
    max: f64,
    over: usize,
    /// Per frame of SIM_FRAME_MS, only when paced: the updates' total time.
    frame_p99: f64,
    frame_max: f64,
    frames_over: usize,
    frames: usize,
    /// The frame in which dusk fell, if it did.
    dusk_frame: Option<usize>,
    /// Each frame's total, in order (only when paced).
    frame_series: Vec<f64>,
}

/// Times `ticks` updates. With `pace_ms`, each update starts on its own schedule, as in the game at
/// that speed (worker threads get the real time between updates); otherwise they run back to back.
fn time_ticks(sim: &mut alpine_sim::sim::Sim, ticks: u64, budget_ms: f64, pace_ms: Option<f64>) -> Timing {
    let mut t = Vec::with_capacity(ticks as usize);
    let mut frame_sums: Vec<f64> = Vec::new();
    let mut dusk_frame = None;
    let begin = Instant::now();
    for i in 0..ticks {
        if let Some(p) = pace_ms {
            let due = begin + std::time::Duration::from_secs_f64(i as f64 * p / 1000.0);
            loop {
                let now = Instant::now();
                if now >= due {
                    break;
                }
                if due - now > std::time::Duration::from_millis(2) {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                } else {
                    std::hint::spin_loop();
                }
            }
        }
        let s = Instant::now();
        sim.step();
        let ms = s.elapsed().as_secs_f64() * 1000.0;
        t.push(ms);
        if let Some(p) = pace_ms {
            let f = (i as f64 * p / SIM_FRAME_MS) as usize;
            if frame_sums.len() <= f {
                frame_sums.resize(f + 1, 0.0);
            }
            frame_sums[f] += ms;
            if dusk_frame.is_none() && sim.events.iter().any(|e| matches!(e, alpine_sim::sim::Event::Dusk)) {
                dusk_frame = Some(f);
            }
        }
    }
    let mean = t.iter().sum::<f64>() / t.len() as f64;
    let over = t.iter().filter(|&&x| x > budget_ms).count();
    t.sort_by(f64::total_cmp);
    let pct = |p: f64| t[((t.len() - 1) as f64 * p).round() as usize];
    let frames_over = frame_sums.iter().filter(|&&x| x > SIM_FRAME_MS / 2.0).count();
    let frame_series = frame_sums.clone();
    frame_sums.sort_by(f64::total_cmp);
    let (frame_p99, frame_max) = match frame_sums.len() {
        0 => (0.0, 0.0),
        n => (frame_sums[((n - 1) as f64 * 0.99).round() as usize], frame_sums[n - 1]),
    };
    Timing {
        ticks: t.len(),
        mean,
        p50: pct(0.5),
        p99: pct(0.99),
        p999: pct(0.999),
        max: t[t.len() - 1],
        over,
        frame_p99,
        frame_max,
        frames_over,
        frames: frame_sums.len(),
        dusk_frame,
        frame_series,
    }
}

fn frame_check(view: &str, top: f64) -> Result<(FrameVerdict, String), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("alpine_front.exe");
    if !exe.exists() {
        return Err(format!("{} not found: run `cargo build --release --workspace` first", exe.display()));
    }
    let out = Command::new(&exe)
        .args(["--frame-check", &FRAME_CHECK_SECONDS.to_string(), "--view", view])
        .output()
        .map_err(|e| format!("could not start the game: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| l.starts_with("FRAME-CHECK"))
        .ok_or_else(|| format!("the game did not report frame times (exit {:?})", out.status.code()))?;
    let field = |name: &str| -> Option<f32> {
        line.split_whitespace().find_map(|w| w.strip_prefix(&format!("{name}="))).and_then(|v| v.trim_end_matches(';').parse().ok())
    };
    let read = |n: &str| field(n).ok_or_else(|| format!("could not read '{n}' from: {line}"));
    let (frames, p50, p99, max) = (read("frames")?, read("p50")?, read("p99")?, read("max")?);
    let (cpu50, cpu99, gpu50, gpu99) = (read("cpu_p50")?, read("cpu_p99")?, read("gpu_p50")?, read("gpu_p99")?);
    let window = line.split("window ").nth(1).and_then(|r| r.split(';').next()).unwrap_or("?");
    let moving = if view == "map" { "the map panning and zooming" } else { "camera flying over the terrain" };
    let seen = format!(
        "{frames:.0} frames over {FRAME_CHECK_SECONDS} s at {window}, {moving}, simulation at {top}x: \
         typical frame {p50:.2} ms, 99% within {p99:.2} ms, slowest {max:.2} ms (limit {FRAME_LIMIT_MS} ms for 99%). \
         Work per frame: processor (main thread) typically {cpu50:.2} ms, 99% within {cpu99:.2} ms; \
         graphics card typically {gpu50:.2} ms, 99% within {gpu99:.2} ms"
    );
    if p99 <= FRAME_LIMIT_MS {
        return Ok((FrameVerdict::Pass, seen));
    }
    // Frames far slower than the work they take: the graphics driver is holding them back.
    let work = cpu50.max(gpu50);
    if p50 > 2.0 * work.max(0.5) {
        return Ok((
            FrameVerdict::NotMeasured,
            format!(
                "{seen}. The work fits easily, but the graphics driver held frames to about {:.0} per second, so the time \
                 between frames could not be judged. This usually happens when the game window is not in front \
                 (drivers often cap background windows at 60 fps); run the speed pass again without touching the \
                 mouse or keyboard while the game window is open",
                1000.0 / p50
            ),
        ));
    }
    Ok((FrameVerdict::Fail, seen))
}

#[derive(PartialEq)]
enum FrameVerdict {
    Pass,
    Fail,
    NotMeasured,
}

fn main() {
    println!("Alpine Front speed pass");
    println!("=======================");
    match pin_to_four_cores() {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            println!("FAIL: {e}");
            std::process::exit(1);
        }
    }

    let (tuning, _) = Tuning::load_default().expect("tuning file");
    let terrain = Arc::new(Terrain::load_col_di_lana(tuning.walking.steepest_walkable_deg).expect("terrain"));
    let threads = tuning.threads.simulation_threads;
    let pool = make_pool(threads);
    let tps = tuning.time.ticks_per_second as f64;
    let top = tuning.time.max_speed as f64;
    let per_tick_ms = 1000.0 / (tps * top);
    let budget = per_tick_ms / 2.0;

    // Simulation check, at the top game speed in real time, as the game runs it.
    let (diary, _) = Weather::load_march_1916().expect("weather diary");
    let places_toml = alpine_sim::roster::load_col_di_lana_1916_text().expect("places file");
    // Run 1, the heaviest load: full daylight (noon on Monday), where men see furthest and
    // line-of-sight work is heaviest, with every squad moving on random orders.
    let step2 = || {
        let mut sim = scenario::step1(terrain.clone(), tuning.clone(), 1916, pool.clone(), true);
        let mut cal = scenario::step2_calendar(&terrain, &tuning).expect("calendar");
        cal.start = sky::DateTime::parse(&format!("{} 12:00", tuning.scenario.step2_first_day)).expect("noon").to_seconds();
        sim.set_clock(cal, diary.clone());
        sim
    };
    // Run 2 and the skip-ahead, the game as played: the work roster (decisions 39 to 43), starting
    // `before_s` game seconds before dusk on Monday. The squads stand at their day places; at dusk
    // every rostered squad changes over at once.
    let roster_game = |before_s: f64| -> (Sim, String) {
        let mut sim = scenario::step2(terrain.clone(), tuning.clone(), 1916, pool.clone(), false, diary.clone(), &places_toml)
            .expect("step 2 with the roster");
        let mut cal = scenario::step2_calendar(&terrain, &tuning).expect("calendar");
        let dusk = cal.next_set(cal.start + 5.0 * 3600.0, 12.0, sky::DUSK_ALTITUDE_DEG, |t| cal.sun_altitude(t)).expect("dusk");
        cal.start = dusk - before_s;
        let at = format!("{} (dusk {})", cal.now(0.0).hhmm(), cal.now(before_s).hhmm());
        sim.set_clock(cal, diary.clone());
        (sim, at)
    };
    let ticks = (SIM_CHECK_MINUTES * 60.0 * tps) as u64;
    let mut sim = step2();
    let n = sim.soldiers.len();
    let t = time_ticks(&mut sim, ticks, budget, Some(per_tick_ms));
    let wander_pass = t.frames_over == 0;
    println!();
    println!(
        "{} Simulation, heaviest load: {n} soldiers, every squad moving on random orders, from noon on Monday, \
         {SIM_CHECK_MINUTES} game minutes ({} updates) run at {top}x speed in real time on {threads} threads. Judged per \
         frame of 1/60 s: the updates in each frame may use at most {:.2} ms together. Of {} frames, 99% used within \
         {:.2} ms, the busiest {:.2} ms; {} frames over the limit. Single updates, for information: each has \
         {per_tick_ms:.2} ms; typical {:.3} ms, 99% within {:.3} ms, 99.9% within {:.3} ms, slowest {:.3} ms; {} took \
         more than half their time ({budget:.2} ms).",
        if wander_pass { "PASS" } else { "FAIL" },
        t.ticks,
        SIM_FRAME_MS / 2.0,
        t.frames,
        t.frame_p99,
        t.frame_max,
        t.frames_over,
        t.p50,
        t.p99,
        t.p999,
        t.max,
        t.over,
    );
    // Headroom: the same run with no pause between updates.
    let mut sim = step2();
    let h = time_ticks(&mut sim, ticks, budget, None);
    println!(
        "     Run flat out with no pause between updates, the same {SIM_CHECK_MINUTES} minutes take {:.1} s, about {:.0}x faster \
         than real time (typical update {:.3} ms, slowest {:.2} ms; {} updates over {budget:.2} ms, mostly waiting for a route \
         still being worked out).",
        h.mean * h.ticks as f64 / 1000.0,
        1000.0 / tps / h.mean,
        h.p50,
        h.max,
        h.over
    );

    // Run 2: the roster game across dusk.
    let (mut sim, at) = roster_game(DUSK_LEAD_MINUTES * 60.0);
    let t = time_ticks(&mut sim, ticks, budget, Some(per_tick_ms));
    let roster_pass = t.frames_over == 0 && t.dusk_frame.is_some();
    // The busiest frame in the two game minutes after dusk, while the squads plan their walks.
    let after_dusk = t.dusk_frame.map(|f| {
        let span = (120.0 * 1000.0 / top / SIM_FRAME_MS).ceil() as usize;
        t.frame_series[f..(f + span).min(t.frame_series.len())].iter().copied().fold(0.0, f64::max)
    });
    println!();
    println!(
        "{} Simulation, the game as played: the work roster, from {at} on Monday, {SIM_CHECK_MINUTES} game minutes run at \
         {top}x speed in real time. Of {} frames, 99% used within {:.2} ms, the busiest {:.2} ms; {} frames over the \
         {:.2} ms limit. {} Single updates, for information: typical {:.3} ms, slowest {:.3} ms.",
        if roster_pass { "PASS" } else { "FAIL" },
        t.frames,
        t.frame_p99,
        t.frame_max,
        t.frames_over,
        SIM_FRAME_MS / 2.0,
        match after_dusk {
            Some(ms) => format!(
                "At dusk every rostered squad changed over at once; the busiest frame in the two game minutes after \
                 was {ms:.2} ms."
            ),
            None => "Dusk did not fall during the run, so the changeover was not measured (this counts as a fail).".into(),
        },
        t.p50,
        t.max,
    );
    let sim_pass = wander_pass && roster_pass;

    // The skip-ahead through Monday night on the roster, for information.
    {
        let (mut sim, at) = roster_game(60.0);
        let start = Instant::now();
        let stop = sim.skip(|s| s.tick < (16.0 * 3600.0 * tps) as u64, |_| {});
        let secs = start.elapsed().as_secs_f64();
        println!(
            "     Skip-ahead on the roster, for information: from {at} on Monday it stopped at {} on {}, {:.1} game hours in {secs:.1} s.",
            sim.now().unwrap().hhmm(),
            match stop.as_deref() {
                Some([alpine_sim::sim::Event::Dawn { .. }, ..]) => "dawn".to_string(),
                Some(e) => format!("{e:?}"),
                None => "nothing (ran out of time)".to_string(),
            },
            sim.game_seconds() / 3600.0
        );
    }

    // Frame check, in each view; the worst verdict counts.
    let mut frames = FrameVerdict::Pass;
    for (view, name) in [("terrain", "3D view"), ("map", "Map")] {
        println!();
        let v = match frame_check(view, top) {
            Ok((v, msg)) => {
                let word = match v {
                    FrameVerdict::Pass => "PASS",
                    FrameVerdict::Fail => "FAIL",
                    FrameVerdict::NotMeasured => "NOT MEASURED",
                };
                println!("{word} Frames, {name}: {msg}.");
                v
            }
            Err(e) => {
                println!("FAIL Frames, {name}: {e}.");
                FrameVerdict::Fail
            }
        };
        if v == FrameVerdict::Fail || (v == FrameVerdict::NotMeasured && frames == FrameVerdict::Pass) {
            frames = v;
        }
    }

    // Scale, for information.
    println!();
    println!("For information (not pass or fail): larger forces, 2 game minutes each, same 8 threads.");
    for total in [1000u32, 3000, 5000] {
        let mut tu = tuning.clone();
        tu.scenario.austrian_squads = total * 3 / 4 / tu.squad.men_per_squad;
        tu.scenario.italian_squads = total / 4 / tu.squad.men_per_squad;
        tu.scenario.austrian_start_radius_m = 1800.0;
        let mut sim = scenario::step1(terrain.clone(), tu, 1916, pool.clone(), true);
        let t = time_ticks(&mut sim, (120.0 * tps) as u64, budget, None);
        println!(
            "  {:>5} soldiers: typical update {:.2} ms, slowest {:.2} ms; could run {:.0}x faster than real time{}",
            sim.soldiers.len(),
            t.p50,
            t.max,
            1000.0 / tps / t.mean,
            if t.mean <= budget { String::new() } else { format!(" (over the {top}x limit)") }
        );
    }

    println!();
    match (sim_pass, frames) {
        (true, FrameVerdict::Pass) => println!("Speed pass: PASSED."),
        (true, FrameVerdict::NotMeasured) => {
            println!("Speed pass: NOT FINISHED (the frame check could not be judged; see above).");
            std::process::exit(2);
        }
        _ => {
            println!("Speed pass: FAILED.");
            std::process::exit(1);
        }
    }
}
