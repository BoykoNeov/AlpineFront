# Decision log

Why the design is what it is. Each entry: what was chosen, what else was on the table, and why.
The current design itself lives in [design-brief.md](design-brief.md); change that first when a
decision changes, then add an entry here.

## 2026-11-17 — Review of the first brief

The first brief was pressure-tested and twelve open points were decided one at a time.

| # | Topic | Chosen | Rejected | Why |
| --- | --- | --- | --- | --- |
| 1 | Location | Col di Lana | Lagazuoi; an invented position on real terrain | Rounded summit fits the heightmap terrain model; a full season arc (attacks, then the mine); Monte Sief next door for later growth. Lagazuoi's ledges and overhangs need a far more complex terrain system. An invented place loses the "this really happened" hook |
| 2 | Target hardware | Strong gaming PC (8 cores, 32 GB, RTX 3070 class) with a speed pass after every build step | Mid-range 5-year-old PC; adding Steam Deck; deciding later | Most headroom for soldier count and terrain. The speed pass stops the game quietly coming to need the very fast development PC |
| 2a | Speed check method | Simulation limited to 8 of 16 threads, pinned two to a core on 4 cores; each update uses at most half its time; frame-time limit sized for a 3070 | "Limit to 8 cores" (the development PC has exactly 8, so it limits nothing); letting Windows place 8 threads (spreads over all 8 cores, same problem); testing on a real older PC (only one PC is available) | A check that actually runs on a smaller machine than the target. The margins are estimates |
| 3 | First playable side | Austria-Hungary, the summit defenders | Italy first; both at once | Defenders live on the mountain all season, so survival, building and the dread of the enemy mine are the core, as the vision says. Italy comes second, built around digging the mine |
| 4 | Core loop | Continuous time plus a daily orders moment at dawn | Purely continuous; separate management and battle modes | Gives quiet weeks real choices (work roster, patrols, building, requests) and matches how a company was run. Separate modes would break continuous time |
| 5 | Soldier model | Medium, built in layers | Lean (a dozen values); deep (full relationship web, per-task skills) | Real stories at a cost that still scales to thousands. Rule: nothing goes in unless it changes what happens and the player can see it |
| 6 | Build order | A rough version of the whole game as step 2, then deepen each part | Combat first (original order); survival first | Finds out early whether the season game is fun. Accepts roughly 10–20% throwaway work |
| 7a | Simulation base | Plain Rust, with Bevy only drawing a snapshot | Bevy's own data system with a pinned run order | Exact replays hold by design, and Bevy upgrades cannot change simulation behaviour |
| 7b | Replay promise | Same machine and game version; saved games are full snapshots | Any machine (needs whole-number maths) | Enough for bug hunting. Upgrade only if multiplayer ever becomes a goal |
| 8 | Who sets test expectations | Claude drafts, director approves, then locked | Director writes them; Claude owns them | Stops the code and its own pass marks being tuned together, which would make the tests prove nothing |
| 9 | Screens | egui now, final look decided before the campaign step | egui for good; Bevy's own interface system | Speed now, and no styling wasted on screens that may change |
| 10 | Default realism | Level 3, with level 4 as the historical setting | Level 2; level 4 | Shows the "map shows reports, not reality" idea without the harshness of being cut off. Revisit when the rough game is playable |
| 11 | Unit look | Shapes when zoomed out, simple figures up close | Figures only; shapes only | Readable at every zoom, no artist needed |
| 12 | Cut-off units | Bounded initiative, reporting back on contact | Strict obedience; full initiative | Realistic and story-rich without leaning everything on the riskiest system, the officer AI |

### Ideas raised but not yet adopted

To check against sources before use: relief rotations bringing strangers into the company;
recruitment by valley, so one disaster hits one village; language barriers inside the
Austro-Hungarian army as a cause of failed orders; firewood scarcity above the tree line; most
supply and digging at night; snow blindness and frostbite; the silence when enemy digging stops; the
cableway as the position's weak point; reports slanted by officer traits; the mass avalanches of
December 1916 as a scenario.

## 2026-11-17 — Build step 1 settings

| # | Topic | Chosen | Rejected | Why |
| --- | --- | --- | --- | --- |
| 13 | Map size | 6 × 6 km centred on the summit | 4 × 4 km; 10 × 10 km | Covers the summit, Monte Sief, the north slopes to Settsass and the south slopes the Italian attacks came up. 4 km cuts off the valley floor; 10 km is mostly empty scenery that eats the speed margin. The sector map in step 10 re-runs the same script on a larger area |
| 14 | Elevation data | TINITALY 1.1, 10 m grid (INGV, CC BY 4.0) | Copernicus 30 m (too coarse for line of sight between soldiers); Veneto regional 10 m (same resolution, harder to download) | Free, 10 m, one script fetches it. Checked: Col di Lana 2,460 m and Monte Sief 2,425 m sit where the maps put them |
| 15 | Who is on the map | About 150 Austro-Hungarians and 50 Italians, spotting only; the player orders Austrian squads by clicking, Italians follow seeded patrols | One side only; both sides fully automatic | Line of sight gets a visible job that can be checked by eye, and the step-2 raid builds on it. Fully automatic would leave no way to set up a situation to test |
| 16 | Frame limit | 30 fps on an RTX 3070, taken as 11 ms per frame on the development PC | 60 fps (about 5.5 ms); 60 fps with extra margin (about 4 ms) | Director's choice: room for terrain detail and many soldiers, as is normal for strategy games |
| 17 | Speed for the simulation check | 16×, the top speed in step 1; raised in step 2 | 60× from the start; 4× | A meaningful bar now, set properly once there is a real week to fast-forward |
| 18 | Git | Finished work goes straight to main, committed and pushed at the end of each batch | One branch per build step | Simplest for a one-person project with no code review |
| 19 | Tests that depend on the summit's shape | Describe today's ground; re-measured and re-approved when the pre-1916 summit is rebuilt | Tying each test to a terrain-file version with automatic skipping; rebuilding the summit before step 1 | Nothing is decided early, and the rebuild stays where the build order put it |
| 20 | What the simulation speed check measures | The simulation running in real time at the top game speed; the flat-out run is reported for information | The flat-out run as pass/fail (step 1 would fail until no update ever waits for a route) | Measures what the player feels. Decided after the first run failed on 50 ms waits for routes. The waits return when step 2 raises the top speed, so step 2 has to deal with them (done: decision 30) |
| 21 | Route-finding | One route per squad, covering only the ground around its men and their way | One whole-map route per destination, shared by every squad going there (the brief's original wording); both combined now | Cheaper in the common case of short moves; all locked tests unchanged. Sharing comes back only if large forces show the need |
| 22 | When the pre-1916 summit is rebuilt | At the start of step 2 | With the mines (step 8); with craters (step 5) | Step 2's week and night raid happen on the summit, so its shape decides cover and sight lines where play happens; tuning on post-war ground would be redone |

Technical choices made while building step 1 (Claude's call, recorded so they can be revisited):

- **Terrain detail.** The whole 6 km map is drawn at full 10 m detail, split into chunks, with no
  coarser distant versions yet. At this size full detail is cheap, and it guarantees that line of
  sight uses exactly the ground you see. The brief's level-of-detail system arrives with the larger
  map in step 10.
- **Earth curvature counts in line of sight**, with the standard allowance for air bending light. It
  hides about 1.7 m at 5 km, which is more than a standing man.
- **Line of sight is exact**, not sampled: it checks the ground at every place the sight line crosses
  a triangle edge, and between those places neither ground nor sight line can bend. The same method
  decides whether a man can walk a straight line, so a sliver of cliff between samples can't trap him.
- **Random numbers come from a small generator written into the project** (xoshiro256**), not a
  library: its sequence can never change under us and its state is four numbers, easy to save.
- **Following a route.** A man joins his squad's route at the best nearby point, then always steps to
  a point strictly closer in walking time, so he can't go round in circles. Earlier versions that
  re-chose the next step freely left men oscillating for hours near steep ground. Men keep right
  when meeting head-on, and a man held up for a second ignores personal space until he moves again.
- **Walking speed** uses Tobler's hiking rule at 70% for a laden soldier. With it the locked 1,243 m
  climb takes about 4.5 hours, against 4.4 by the Alpine trail-sign walking-time rule (DIN 33466).
- **Speed pass runs the simulation in real time at 16×.** Run flat out, a squad's route (worked out
  on another thread during the one game second before it moves) gets almost no real time, and the
  update that waits for it took about 50 ms. In the game at 16× that second is 62 ms of real time.
  So the pass/fail check paces the simulation as the game does, and the flat-out run is reported
  as headroom. Routes now cover only the area around the squad and its way, not the whole map.
- **Frame check is protected from driver caps.** The development PC's graphics driver held frames
  at 119 per second (and at 60 when the game window was behind another), whatever the game did.
  The check now also reports work per frame (about 1.2 ms on the processor and 0.3 ms on the
  graphics card in step 1), and says "not measured" rather than failing when a cap is to blame.
  Known weakness: the processor figure covers only the game's main thread, not Bevy's drawing
  thread or the driver, so a slowdown there could show as "not measured" instead of a failure.
  Treat "not measured" as "run again with the window in front", never as a pass.
- **Camera.** Bevy's built-in free camera uses the right mouse button to grab the mouse, which
  clashes with right-click orders, so the game has its own: right-drag looks, right-click orders.

### Sources used

- [Col di Lana (Wikipedia)](https://en.wikipedia.org/wiki/Col_di_Lana)
- [Mines on the Italian front, World War I (Wikipedia)](https://en.wikipedia.org/wiki/Mines_on_the_Italian_front_(World_War_I))
  — the Italian mine of 17 April 1916 (about 5 tonnes) and the earlier Austrian counter-mine of 6 April 1916
- [Smithsonian: Climbing the Via Ferrata](https://www.smithsonianmag.com/travel/climbing-the-via-ferrata-8081616/)
  — summit roughly 90 feet lower after the blast (withdrawn: see decision 28)
- [Lagazuoi open-air museum](https://lagazuoi.it/Eng/page7-Open-air-Museum-of-the-Lagazuoi)
- [Veneto terrain model (10 m mountain grid)](https://www.regione.veneto.it/web/agricoltura-e-foreste/modello-digitale-del-terreno)
- [Veneto laser-scan data](https://www.regione.veneto.it/web/agricoltura-e-foreste/modelli-digitali-delle-chiome)
- Elevation data used in the game (CC BY 4.0, citation required): Tarquini S., I. Isola, M. Favalli,
  A. Battistini, G. Dotta (2023). TINITALY, a digital elevation model of Italy with a 10 meters cell
  size (Version 1.1). Istituto Nazionale di Geofisica e Vulcanologia (INGV).
  https://doi.org/10.13127/tinitaly/1.1

## 2026-11-17 — A second view: the 2D map

The director asked for a 2D view with the same capabilities as the 3D one. Decided one point at a
time.

| # | Topic | Chosen | Rejected | Why |
| --- | --- | --- | --- | --- |
| 23 | What the 2D view is | A separate map drawn as a period staff map; everything playable in both views | The 3D world seen from straight above (Claude's recommendation: about a day's work, nothing drawn twice); a small overview map in a corner; waiting until the sector seat | A separate map can later show things the 3D view does not: symbols, names, only what the reports say. Cost accepted: everything drawn on the ground is drawn twice. Clicks, orders and panels are shared, so only the drawing doubles |
| 24 | When it is built | Now, after step 1 and before step 2 | Inside step 2 (makes the deliberately crude step bigger); at step 9 with the final look (every feature from steps 2 to 8 retrofitted) | The view code is still small, and step 2 then builds each new feature for both views from the start |
| 25 | How it looks now | The period look from the start: paper, hand-drawn style relief, lettered names, period-style squad symbols | Plain now, period look before step 9 | "Plain now" was chosen first, then reversed: a plain separate map looks much like the cheaper top-down view while costing the same double work, so the separate map only pays if it looks like what it is. Revisited with the other screens before step 9 |
| 26 | Soldiers on the map | One symbol per squad when zoomed out, each man as a dot up close | Squad symbols only (a straggler would show only in 3D); every man as a dot (cluttered at 200, unreadable at thousands) | Same rule as the 3D view (shapes far, figures near), readable at every zoom, and it scales to the sector seat. Clicking a squad symbol also picks its leading man, so "what this man can see" still has an observer |
| 27 | Colours of the sides | Own troops (Austro-Hungarian) blue, Italians red, in both views; the 3D view's orange becomes red | Red for own troops | No fixed rule existed: maps of the time used either way round. German trench maps drew their own positions in blue, which also matches today's habit, so players read it without a legend |

Technical choices for the map (Claude's call, recorded so they can be revisited):

- **Same ground as the simulation.** The map reads the simulation's heights and splits each grid
  square into the same two triangles, so contour lines and steep ground on the map are exactly what
  the soldiers walk on.
- **Names only where the height data proves the place.** Col di Lana (the summit at the map centre),
  Settsass (2,575 m, the highest point on the map, 2.5 km north) and Monte Sief (2,425 m, 610 m
  north-west along the ridge, the height decision 14 checked).
  Villages and passes wait until their positions are checked against old maps.
- **Lettering font** is Old Standard (SIL Open Font License), modelled on book type of around 1900,
  kept in the repository with its licence file (`crates/alpine_front/src/fonts/`).
- **Squad flags** copy the practice of pinning little flags into wall maps; they are not a copy of
  any one army's official symbol set.
- **Forest on the map is decoration**, painted below about 1,900 m like the 3D view's green: the
  simulation has no forest yet.
- **A click check** (`ALPINE_CLICK_CHECK=1`) clicks a squad, orders it, on the map zooms with the
  wheel, moves with W and right-drags, and switches views with M, through the same code as the real
  mouse and keys, so both views are proven playable without a person.

### Sources used

- [NYPL Map Division: WWI maps](https://live-legacy-admin.nypl.org/blog/2015/05/15/wwi-map-division)
  — German trench maps overprinted German positions in blue
- [Habsburg WW1: Galicia 1915](https://ww1.habsburger.net/en/node/5811) and
  [Leo Baeck Institute: Bardach's map](https://www.lbi.org/web-exhibits/WWI/ExhibitWWIPart3_Map.html)
  — other maps of the time used the colours differently, so there was no single convention

## 2026-11-17 — The summit before the mine

Step 2 starts by rebuilding the summit as it was before the Italian mine of 17 April 1916.

| # | Topic | Chosen | Rejected | Why |
| --- | --- | --- | --- | --- |
| 28 | How the pre-1916 summit is rebuilt | Fill the mine's crater only; today's high point (2,460 m in the elevation data) stays the top | Fill the crater and make the old top 10 m higher; take "about 90 feet (27 m) lower" at face value | The crater is the only change a solid source supports. Raising the top would be invented: +10 m leaves two sight tests passing by under a metre, +27 m breaks two locked tests (summit to Monte Sief, and the rounded top). Filling the crater changes no test result at all. If a height from before the war turns up, the rebuild script takes it as one number |

**Correction to earlier notes.** The scenario-test notes and the step-1 sources said the
mine took "roughly 27 m" off the summit. That came from one travel article (Smithsonian, "about 90
feet lower"). The list of Italian-front mines, drawn from Striffler's study of the 34 mines on the
Tyrolean front, gives an oblong double crater 35 m long, 25 m wide and 12 m deep; the Italian
Wikipedia gives 30 × 55 m and 12 m deep. A crater of that size is not a 27 m lower summit, so the
27 m figure is withdrawn.

What was checked (Claude's work, recorded so it can be revisited):

- **No pre-war height found.** Searches in English, Italian and German for the summit's height on
  maps from before the war found nothing. Today's sources give 2,452, 2,462 or 2,465 m.
- **Where the crater is.** At 10 m detail the crater shows only as an oblong hollow about 6 m deep,
  30 to 50 m north-north-west of today's high point, about the recorded size. That is taken as the
  crater; it is a reading of the data, not a surveyed position.
- **How it is filled.** `tools/rebuild_summit.py` fills an oval of 40 × 52 m around that hollow with
  the smoothest surface that meets the ground around it, never lowering any point. The oval is a
  little larger than the recorded crater because the 10 m grid smears the hole. `prepare_terrain.py`
  runs it every time it builds the map, always on the untouched elevation data. 14 grid points
  rise, by at most about 5 m.
- **Tests re-measured.** All 11 locked tests were run on the rebuilt summit and on the two rejected
  versions. On the rebuilt summit every result, down to the margins, is identical to today's
  ground, so no expectation changes.
- **Replays now record the ground.** A replay stores a fingerprint of the map it was recorded on and
  refuses to play on different ground, instead of reporting a false "diverged". Replays saved before
  this change can no longer be played.
- **Finer elevation data** (the open question about Veneto's laser scan) would sharpen today's
  crater but cannot show the ground before 1916, so it was not pursued for this.

### Sources used

- [Mines on the Italian front, World War I (Wikipedia)](https://en.wikipedia.org/wiki/Mines_on_the_Italian_front_(World_War_I))
  — the 17 April 1916 mine: 5,020 kg, an oblong double crater 35 × 25 m and 12 m deep, citing
  R. Striffler, *Die 34 Minensprengungen an der Tiroler Gebirgsfront 1916–1918* (1993)
- [Col di Lana (Italian Wikipedia)](https://it.wikipedia.org/wiki/Col_di_Lana) — crater 30 × 55 m,
  12 m deep
- [Smithsonian: Climbing the Via Ferrata](https://www.smithsonianmag.com/travel/climbing-the-via-ferrata-8081616/)
  — the "about 90 feet lower" claim, now withdrawn

## 2026-11-17 — Step 2: game speed

At 16×, the top speed through step 1, one game week takes about 10½ real hours to play.

| # | Topic | Chosen | Rejected | Why |
| --- | --- | --- | --- | --- |
| 29 | Speeding up quiet stretches | Normal speeds up to 64×, plus a skip-ahead that runs as fast as the computer can until dawn or until something happens, slowing on events. The speed check holds 64× to its limit; the skip-ahead's speed is reported for information | One high top speed of a few hundred × with no skip (the speed check would first need the occasional slow update while squads plan routes fixed); keeping 16× and adding only the skip (nothing between watching and jumping) | 64× lets a quiet night pass in about 10 minutes while still watchable; the skip-ahead jumps dead hours without making every update meet a much tighter limit. Replaces decision 17's step-1 bar. The skip-ahead is built with the dawn orders screen, which brings the game clock and the events it stops on. Measured afterwards: 64× also failed at first on the route waits, which the rejected option's note had blamed only on the higher speed; see decision 30 |
| 30 | How the speed check judges the simulation at 64× | The squad leader's planning pause after an order goes from 1 to 4 game seconds, and the check judges each frame's batch of updates (the updates falling in each 1/60 s of real time must take at most 8.3 ms together); single slow updates are reported, not failed | Keeping the per-update rule and tracking down the last slow updates (unknown effort; they may be Windows hiccups on a busy machine); dropping the top speed to 32× (passes only narrowly; a quiet night takes 20 minutes) | At 64× each update has 0.78 ms. With the 1 s pause, long routes (up to 40 ms of work) were not ready when needed and the game stalled; 4 s gives them about 60 ms of real time at 64×, and removed every stall in the test. Nine updates of about 1 to 1.3 ms remained; at 64× the game runs about 10 updates per frame drawn, so one of them among ten 0.1 ms updates cannot be seen. A real stutter still fails. Same idea as decision 20: measure what the player feels. A 4 s pause is also closer to a leader taking stock before moving off |

## 2026-11-17 — Step 2: the week

| # | Topic | Chosen | Rejected | Why |
| --- | --- | --- | --- | --- |
| 31 | When the step-2 week is set | Monday 6 to Sunday 12 March 1916 | Autumn 1915 (the big Italian attacks lean on combat that arrives only in step 3); summer 1915 (no falling snow, which step 2 includes; short nights for the raid) | Late winter exercises every step-2 item: cold, hunger, falling snow, carrying parties through snow and a long night for the raid. The Italians had been tunnelling toward the summit since mid-January, so the dread the brief puts at the core is already present. Cold and hunger get tuned at their hardest first |

Worked out for the brief (Claude's work, recorded so it can be revisited):

- **Daylight.** Sunrise and sunset on the summit (46.50° N, 11.96° E) from the standard solar
  formula, for a flat horizon: about 06:42 and 18:05 on 6 March, 06:31 and 18:13 on 12 March. Times
  are Central European Time; Austria-Hungary began summer time only at the end of April 1916.
  Surrounding peaks will delay the first direct sun; that is not modelled.
- **Exact dates.** A Monday-to-Sunday week in early March, about five weeks before the mine. The
  director can move it; only the daylight figures change.

### Sources used

- [The mine placed under Col di Lana (Turismo FVG)](https://www.turismofvg.it/en/108650/the-mine-placed-under-col-di-lana)
  — the decision to mine the summit, January 1916
- [Peppino's battle for Col di Lana (History of War, via PressReader)](https://www.pressreader.com/uk/history-of-war/20180614/282621738408331)
  — tunnelling began in mid-January 1916
