# Alpine Front

A First World War strategy and tactics game set on the Alpine front. Every soldier is simulated as
an individual, and the mountain itself (altitude, snow, rock, cold) is the main opponent. The player
commands an Austro-Hungarian company holding the summit of Col di Lana through a season.

**Status:** build step 1 (pipeline): Col di Lana terrain, 200 soldiers on two sides moving and
spotting each other by line of sight, a free camera, debug overlays and exact replays. No combat.
Added after step 1: a 2D map drawn as a First World War staff map, with everything the 3D view can
do.

Build step 2 (a week on the summit, rough) under way. Done so far: the summit as it was before the
April 1916 mine; the calendar (Monday 6 to Sunday 12 March 1916), with the sun, the moon and a weather
diary setting how far men can see, from 1,500 m by day to 15 to 50 m on a dark night; the dawn orders
screen with a morning report; the skip-ahead; and the work roster: each squad guards a lookout post
or sleeps in a shelter, by day and by night, and men tire while awake (posts and shelters from period
accounts of the summit position, in `data/places/col_di_lana_1916.toml`).

- [Design brief](docs/design-brief.md): what the game is, the agreed decisions, architecture and
  build order
- [Decision log](docs/decisions.md): why each decision was taken

- [Scenario tests](docs/scenario-tests.md): the locked design statements the tests check

Built in Rust with the Bevy engine.

## Running the game

    cargo run --release -p alpine_front

The game opens at first light on Monday 6 March 1916 with the orders screen; press Enter (or click
"Carry on") to start the day. It stops there again every morning at first light. The orders screen
holds the work roster: click "guard" or "rest" for each squad's day and night duty. Squads change over
by themselves at dusk and dawn; the side panel's "Work roster" changes duties at any time. Resting men
are inside their shelter and are not drawn; select their squad from the roster. A squad you send
somewhere leaves its duty until the next dusk or dawn, or until you press "Back to duty".

Options: `--seed N` picks a different starting layout, `--step1` plays the step-1 scenario (no
calendar, always daylight), `--autoplay` sets every squad wandering on
its own, `--replay FILE` plays a saved replay back and checks that it matches, and `--view map`
starts on the map. A replay plays only on the ground it was recorded on; replays saved before the
map changed are refused.

M switches between the 3D view and the map. In both, left-click selects a squad, right-click the
ground sends a selected Austro-Hungarian squad there, Space pauses, 1 to 7 set the speed (1x to
64x), N skips ahead (as fast as the computer can, until first light or until something happens: an
Italian squad nobody had seen for 30 minutes comes into sight, or a squad you sent arrives or cannot
get there; in normal play these show a message, and the side panel's "When something happens"
setting can make them slow the game to 8x or 1x as well), F5 saves a replay into `replays/`, and the panel
on the left switches the overlays.

- 3D view: W A S D move, Q / E down / up, Shift faster, mouse wheel changes camera speed, hold the
  right mouse button and drag to look around. Left-click a soldier to select his squad.
- Map: W A S D or the arrow keys move the map, the mouse wheel zooms at the cursor, Q / E zoom out /
  in, hold the right mouse button and drag to move the map. Zoomed out, each squad is a little flag
  (click it to select the squad); zoomed in, each man is a dot.

## Running the tests

    cargo test --release -p alpine_sim

The scenario tests end in a plain-language report: `--test scenarios` runs the step-1 tests (about
30 s), `--test step2` the calendar, night and skip-ahead tests (about 3 minutes), `--test roster` the
work-roster tests (about 90 s).

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

At 64x, the top speed from step 2 (2026-11-17): passed in two runs, after the squad leader's
planning pause went from 1 to 4 game seconds and the check moved to judging each 1/60 s frame
(decision 30). The busiest frame's simulation work was 5.1 to 7.6 ms against an 8.3 ms limit (99%
within 2.4 ms); 99% of drawn frames within 8.8 ms in both views. Run flat out, the simulation goes
about 1,080x faster than real time.

Step 2, with the calendar and the light model (2026-11-17): passed, measured at noon (full
daylight, the heaviest sight work). The busiest frame's simulation work was 3.2 ms against an 8.3 ms
limit (99% within 2.5 ms); 99% of drawn frames within 8.7 ms in both views. Run flat out the
simulation goes about 905x faster than real time; the skip-ahead crosses a night, dusk to first
light (11.6 game hours), in about 16 s.

Step 2, with the work roster (2026-11-17): passed. The busiest frame's simulation work was 2.7 ms
against an 8.3 ms limit (99% within 2.4 ms); 99% of drawn frames within 8.8 ms in both views. Run
flat out the simulation goes about 960x faster than real time; the skip-ahead crosses a night in
about 15 s. (The check still ran every squad wandering on random orders, without the roster, so
the roster's own work and the place names in the 3D view were not measured; fixed below.)

The roster measured too (2026-11-18, decision 44): passed. The simulation check now has two runs
and both must pass. Every squad moving on random orders at noon: busiest frame 4.3 ms against
8.3 ms (99% within 2.4 ms). The game as played, on the roster across Monday's dusk, when every
squad changes over at once: busiest frame 4.2 ms (99% within 1.2 ms; 1.8 ms at most in the two
game minutes after dusk). The frame check now plays the roster game with the place names drawn
(resting men are in their shelters and not drawn): 99% of frames within 8.6 ms in both views. Run
flat out the simulation goes about 990x faster than real time; the skip-ahead on the roster crosses
Monday night, dusk to first light, in about 15 s.

`ALPINE_CLICK_CHECK=1 target\release\alpine_front.exe --view map` (or `--view terrain`) checks that a
view can be played: it carries on at the dawn orders screen, moves in close to the summit (where the
squads stand at their posts), clicks a squad, orders it, on the map also zooms with the wheel, moves the
map with W and by right-dragging, then switches views with M, tries the skip-ahead with N, and reports PASS or FAIL.

## Lettering

The map's lettering is Old Standard (regular and italic), by the Old Standard Project Authors, used
under the SIL Open Font License 1.1; the licence is in `crates/alpine_front/src/fonts/OFL.txt`.

## Elevation data

The terrain in `data/terrain` is cut from TINITALY by `tools/prepare_terrain.py`, which also fills
the crater of the April 1916 mine to show the summit before the blast (`tools/rebuild_summit.py`,
decision 28). It is used under the CC BY 4.0 licence:

Tarquini S., I. Isola, M. Favalli, A. Battistini, G. Dotta (2023). TINITALY, a digital elevation
model of Italy with a 10 meters cell size (Version 1.1). Istituto Nazionale di Geofisica e
Vulcanologia (INGV). https://doi.org/10.13127/tinitaly/1.1

No license has been chosen yet, so all rights are reserved for now.
