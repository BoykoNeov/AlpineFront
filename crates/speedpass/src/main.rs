//! Speed pass (docs/design-brief.md, Build order): run after every build step on the development PC.
//!
//! 1. Restricts this process (and the game it launches) to 8 logical processors on 4 physical cores,
//!    two per core, read from Windows' own description of the CPU.
//! 2. Simulation check: runs the step-1 scenario with every squad moving and times every update.
//!    At the fastest game speed each update must use no more than half the time it has.
//! 3. Frame check, once per view: launches the game at 2560x1440 with vsync off, flies the camera
//!    over the 3D terrain (then pans and zooms the 2D map) and reads back its frame times. In each
//!    view 99% of frames must finish within 11 ms (30 fps on an RTX 3070, taken as about a third of
//!    the RTX 5090's speed).
//! 4. For information only: how the simulation copes with 1,000, 3,000 and 5,000 soldiers.
//!
//! Build everything in release mode first, then run:
//!   cargo build --release --workspace
//!   target\release\speedpass.exe

use alpine_sim::scenario;
use alpine_sim::sim::make_pool;
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
}

/// Times `ticks` updates. With `pace_ms`, each update starts on its own schedule, as in the game at
/// that speed (worker threads get the real time between updates); otherwise they run back to back.
fn time_ticks(sim: &mut alpine_sim::sim::Sim, ticks: u64, budget_ms: f64, pace_ms: Option<f64>) -> Timing {
    let mut t = Vec::with_capacity(ticks as usize);
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
        t.push(s.elapsed().as_secs_f64() * 1000.0);
    }
    let mean = t.iter().sum::<f64>() / t.len() as f64;
    let over = t.iter().filter(|&&x| x > budget_ms).count();
    t.sort_by(f64::total_cmp);
    let pct = |p: f64| t[((t.len() - 1) as f64 * p).round() as usize];
    Timing { ticks: t.len(), mean, p50: pct(0.5), p99: pct(0.99), p999: pct(0.999), max: t[t.len() - 1], over }
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
    let mut sim = scenario::step1(terrain.clone(), tuning.clone(), 1916, pool.clone(), true);
    let n = sim.soldiers.len();
    let ticks = (SIM_CHECK_MINUTES * 60.0 * tps) as u64;
    let t = time_ticks(&mut sim, ticks, budget, Some(per_tick_ms));
    let sim_pass = t.over == 0;
    println!();
    println!(
        "{} Simulation: {n} soldiers, every squad moving on random orders, {SIM_CHECK_MINUTES} game minutes ({} updates) \
         run at {top}x speed in real time on {threads} threads. Each update has {per_tick_ms:.2} ms, so the limit is {budget:.2} ms. \
         Typical update {:.3} ms, 99% within {:.3} ms, 99.9% within {:.3} ms, slowest {:.3} ms; {} updates over the limit.",
        if sim_pass { "PASS" } else { "FAIL" },
        t.ticks,
        t.p50,
        t.p99,
        t.p999,
        t.max,
        t.over,
    );
    // Headroom: the same run with no pause between updates.
    let mut sim = scenario::step1(terrain.clone(), tuning.clone(), 1916, pool.clone(), true);
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
