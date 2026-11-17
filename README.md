# Alpine Front

A First World War strategy and tactics game set on the Alpine front. Every soldier is simulated as
an individual, and the mountain itself (altitude, snow, rock, cold) is the main opponent. The player
commands an Austro-Hungarian company holding the summit of Col di Lana through a season.

**Status:** build step 1 (pipeline): Col di Lana terrain, 200 soldiers on two sides moving and
spotting each other by line of sight, a free camera, debug overlays and exact replays. No combat.
Added after step 1: a 2D map drawn as a First World War staff map, with everything the 3D view can
do.

- [Design brief](docs/design-brief.md): what the game is, the agreed decisions, architecture and
  build order
- [Decision log](docs/decisions.md): why each decision was taken

- [Scenario tests](docs/scenario-tests.md): the locked design statements the tests check

Built in Rust with the Bevy engine.

## Running the game

    cargo run --release -p alpine_front

Options: `--seed N` picks a different starting layout, `--autoplay` sets every squad wandering on
its own, `--replay FILE` plays a saved replay back and checks that it matches, and `--view map`
starts on the map.

M switches between the 3D view and the map. In both, left-click selects a squad, right-click the
ground sends a selected Austro-Hungarian squad there, Space pauses, 1 to 5 set the speed (1x to
16x), F5 saves a replay into `replays/`, and the panel on the left switches the overlays.

- 3D view: W A S D move, Q / E down / up, Shift faster, mouse wheel changes camera speed, hold the
  right mouse button and drag to look around. Left-click a soldier to select his squad.
- Map: W A S D or the arrow keys move the map, the mouse wheel zooms at the cursor, Q / E zoom out /
  in, hold the right mouse button and drag to move the map. Zoomed out, each squad is a little flag
  (click it to select the squad); zoomed in, each man is a dot.

## Running the tests

    cargo test --release -p alpine_sim

The scenario tests end in a plain-language report.

## Speed pass

After every build step:

    cargo build --release --workspace
    target\release\speedpass.exe

It restricts itself to 8 threads on 4 cores, times the simulation at the top game speed, launches the
game twice to time its frames in the 3D view and on the map, and ends in a plain-language report. Leave the game window in front while
it runs: graphics drivers slow down background windows, and the check then reports "not measured".

Step 1 result (2026-11-17): passed in three runs. Slowest simulation update 1.7 to 3.0 ms against a
3.1 ms limit (typical 0.12 ms); 99% of frames within 8.6 ms against an 11 ms limit.

With the map (2026-11-17): passed. Slowest simulation update 1.4 ms; 99% of frames within 8.6 ms in
both the 3D view and the map.

`ALPINE_CLICK_CHECK=1 target
eleaselpine_front.exe --view map` (or `--view terrain`) checks that a
view can be played: it clicks a squad, orders it, on the map also zooms with the wheel, moves the
map with W and by right-dragging, then switches views with M, and reports PASS or FAIL.

## Lettering

The map's lettering is Old Standard (regular and italic), by the Old Standard Project Authors, used
under the SIL Open Font License 1.1; the licence is in `crates/alpine_front/src/fonts/OFL.txt`.

## Elevation data

The terrain in `data/terrain` is cut from TINITALY by `tools/prepare_terrain.py` and is used under
the CC BY 4.0 licence:

Tarquini S., I. Isola, M. Favalli, A. Battistini, G. Dotta (2023). TINITALY, a digital elevation
model of Italy with a 10 meters cell size (Version 1.1). Istituto Nazionale di Geofisica e
Vulcanologia (INGV). https://doi.org/10.13127/tinitaly/1.1

No license has been chosen yet, so all rights are reserved for now.
