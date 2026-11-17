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

## 2026-11-17 — Step 2: the dawn orders screen, night and the skip-ahead

| # | Topic | Chosen | Rejected | Why |
| --- | --- | --- | --- | --- |
| 32 | What the dawn orders screen holds now | The date, the weather, a morning report of the last 24 hours (men present, enemy sightings summarised, squads that finished a move or could not), and a "Carry on" button. Each later piece (work roster, carrying parties, patrols) adds its own section when it is built | Also choosing each squad's post for the day, with "same as yesterday" (the roster would probably absorb or replace it); also standing instructions (nothing can act on them until steps 3 and 4) | Nothing fake on the screen: the brief's rule is that nothing goes in unless it changes what happens and the player can see it. The roster is designed properly next instead of being guessed at here |
| 33 | Seeing at night | A fuller night model now: the sun's height with twilight, the moon's phase and height, starlight, snow, and cloud set the light, and the light sets the spotting distance | A crude night cut (one number for every night); leaving sight at 1,500 m day and night and hiding night sightings from the report | Measured first: over one night with the step-1 patrols our men saw Italians 53 to 93% of the time, at a median 1,150 to 1,320 m, which is impossible in the dark and would have filled the report and stopped the skip-ahead every few minutes. Director's choice of the fuller model; it brings part of step 7 forward |
| 34 | Where this week's cloud comes from | A weather diary for 6 to 12 March 1916 in a readable data file (`data/weather/march_1916.toml`), cloud cover through the week, invented but plausible; the falling snow piece adds to it | Cloud drawn at random from the seed (harder to test and to tune against); clear skies until the falling snow piece | Editable, the same week every playthrough, and tests can rely on it. No weather record from the summit for that week has been found |
| 35 | How far a man can be spotted in each light | A man in a dark coat against snow: 1,500 m by day (also overcast day), 1,000 m in twilight just after sunset (about 10 lux), 500 m in deep twilight (1 lux), 300 m under a full moon (0.25 lux), 120 m under a quarter moon (0.01 lux), 50 m by clear starlight only (0.002 lux), 15 m on an overcast moonless night (0.0001 lux), changing smoothly in between. Values in `data/tuning.toml` | About twice as far (snow makes figures stand out more); about half as far | Light levels come from published tables; the distances are Claude's estimates, because no unaided-eye measurement for a man against snow was found. The night raid piece will test them in play |
| 36 | What stops the skip-ahead | Dawn; our men seeing an enemy squad not seen in the previous 30 minutes; and a squad we ordered arriving or finding it cannot get there | Dawn and arrivals only (could skip past Italians walking up to the summit); dawn and dusk only (ignores everything) | The skip never runs past anything that matters. By day the step-1 patrols, which are in sight most of the day, would stop it about 20 times in 12 hours; at night, with the light model, never. Daytime skipping is choppy until the Italians behave less like wanderers (the raid piece) |

Worked out while deciding (Claude's work, recorded so it can be revisited):

- **The sky that week.** New moon on 4 March 1916, first quarter on the evening of 11 March, full
  moon on 19 March. Each night of the week a growing crescent sets in the evening, from about 21:10
  on Monday to about 02:40 on Sunday; the rest of each night is moonless. A low-precision solar
  and lunar calculation for the summit agrees with an independent table for Telve (Trentino,
  55 km west) for 11 March 1916: sunrise 06:35 there against 06:33 here, moonrise 09:49 against
  about 10:00, moonset 01:42 against about 01:40, the moon 49% lit against 52%.
- **Light levels used.** Sunset on a clear day about 400 lux, the end of civil twilight (sun 6°
  down) about 3 lux, the end of nautical twilight (12° down) about 0.008 lux, astronomical (18°)
  about 0.0006 lux; full moon about 0.25 lux, quarter moon about 0.01 lux, clear starlight about
  0.002 lux, overcast moonless night about 0.0001 lux. Sources disagree by a factor of two or so;
  these are order-of-magnitude figures.
- **Night range at the measurement.** With a night spotting distance of 150 or 300 m, the step-1
  Italian patrols (which stay at least 1 km from the summit) were never seen at night, and a whole
  night ran headless in about 18 seconds instead of about a minute.

### Sources used

- [Orders of magnitude (illuminance), Wikipedia](https://en.wikipedia.org/wiki/Orders_of_magnitude_(illuminance))
  — full moon, quarter moon, starlight and overcast night
- [Civil twilight, AMS Glossary of Meteorology](https://glossarytest.ametsoc.net/wiki/Civil_twilight)
  — illuminance falling from about 410 to 585 lux to about 2 to 3.5 lux through civil twilight
- [Twilight types, US National Weather Service](https://www.weather.gov/lmk/twilight-types)
  — the twilight stages by sun angle
- [Moon phases March 1916 for Telve (mooninfo.org)](https://www.mooninfo.org/world/italy/292324/moon-phases-march-1916-for-telve.html)
  — the independent check of the sun and moon calculation
- [FM 21-75 Scouting, Patrolling and Sniping (US War Department, 1944)](https://reenactorpro.org/wp-content/uploads/2017/10/FM-21-75-Scouting-Patrolling-and-Sniping.pdf)
  — daylight distances at which troops can be made out, and night observation technique; it gives
  no night distances
- [Performance modeling of electro-optical devices for military target acquisition (arXiv)](https://arxiv.org/pdf/1904.11678)
  — the unaided eye detecting targets up to about 500 m under a full moon, background not stated

## 2026-11-17 — Step 2: the orders moment comes at first light

| # | Topic | Chosen | Rejected | Why |
| --- | --- | --- | --- | --- |
| 37 | When the dawn orders moment comes | First light, the standard meaning of dawn: the sun 6° below the horizon (the same rule as dusk), about 06:12 on Monday to 06:01 on Sunday. Test 12 changed to match | Sunrise, keeping test 12 and rewording test 18 so the last half-hour before sunrise may stop the skip-ahead; dimming the morning twilight so the patrols stay hidden until sunrise (goes against the light figures, only to pass a test) | The first test run found two approved tests in conflict. Measured on all seven nights: no sighting at all between dusk and first light, then one to five sightings each morning between first light and sunrise, Italians 740 to 1,500 m away in the growing light, each stopping the skip-ahead just short of an orders screen at sunrise. At first light the night is over and the sightings happen in play. Troops of the time stood to arms at first light, so it fits as well |

Technical choices made while building the dawn screen (Claude's call, recorded so they can be
revisited):

- **Events only with a calendar.** Dawn, new contacts and arrivals are noted only in scenarios on
  the calendar, so step 1 behaves and checksums exactly as before: its 11 test results are word for
  word the same, and a step-1 replay recorded with the code from before this work
  (`crates/alpine_sim/tests/data/step1_before_step2.ron`) still plays back matching.
- **Slowing on events.** A new contact, an arrival or a squad that cannot get there stops the
  skip-ahead and shows a message naming what happened; what it does to normal play is the player's
  choice (decision 38). First light pauses the game and opens the orders screen. Squads on patrol,
  and Italian squads' own moves, raise no events.
- **The skip-ahead** runs the simulation flat out for up to 12 ms of real time per drawn frame, so
  the screens stay responsive, and stops on the exact update of the event. It calls the same update
  as normal play (test 20 checks the two give identical state for a whole day).
- **The light is worked out every update** from the sun and moon positions (a few microseconds), for
  a flat horizon: surrounding peaks hiding a low sun or moon are not modelled. The moon's light uses
  Allen's rule for how its brightness falls away from full, and is dimmed near the horizon by the
  thickness of air.
- **The orders screen opens when the game starts** on the calendar, but not when the game is
  fast-forwarded at start or run automatically (autoplay, the frame check), which carry on at dawn
  by themselves.
- **Replays keep the weather diary** they were recorded with, as they keep the tuning file, so
  editing the diary does not break them. Old replays without one still load.
- **The "what this man can see" shading** is worked out again whenever the spotting distance changes
  by more than 5%, so it follows the light through dusk and dawn.
- **The speed pass** now measures the step-2 game at noon on Monday (full daylight, where men see
  furthest and sight work is heaviest; every squad moving), and reports how long the skip-ahead
  takes from dusk to first light.

## 2026-11-17 — Step 2: what events do to normal play

| # | Topic | Chosen | Rejected | Why |
| --- | --- | --- | --- | --- |
| 38 | What a new contact or an arrival does to normal play | A setting the player chooses in the side panel: only show a message (the default), slow to 8×, or slow to 1×. The skip-ahead always stops, and first light always opens the orders screen | Always dropping to 1× (as first built); always slowing to 8× (Claude's recommendation); slowing only for enemies within some distance | Measured: by day the current patrols give about 20 new sightings per half-day, so dropping to 1× would interrupt play at 64× about every 30 real seconds. Director's choice: let the player decide, and by default do not slow down. Replaces the brief's "auto-slows on events" for normal play; a setting of the screen, not the simulation, so replays are unaffected |

## 2026-11-17 — Step 2: the work roster

| # | Topic | Chosen | Rejected | Why |
| --- | --- | --- | --- | --- |
| 39 | Which duties the first roster has | Guard and rest, with simple tiredness added now: guards watch from lookout posts and grow tired; resting men sleep in shelters, out of sight both ways, and recover; tired men spot at shorter distances and walk slower. Carrying arrives with the carrying-parties piece; digging waits for building (step 6) | Guard and off duty without tiredness; guard, rest and carrying built as one piece; a "dig" duty that does nothing yet | Nothing goes in unless it changes what happens and the player can see it. Without tiredness, keeping everyone on guard is always best, so there is no choice to make. Building carrying at the same time needs supply stocks and a depot, and delays the first thing the director can judge |
| 40 | Where the lookout posts and shelters are | From period sources: where the Austro-Hungarian posts, shelters and caverns on Col di Lana stood in early 1916, researched first; invented but believable places only where the sources are silent, marked as such in the data file | Invented but believable from the start (Claude's recommendation); the player placing them on the first morning | Director's choice: ground the position in history from the start. Kept in a readable data file and shown with names in both views |
| 41 | How often duties are set | Each squad gets a day duty and a night duty at the dawn orders screen, "same as yesterday" filled in; they switch by themselves at dusk and dawn. The roster can also be changed at any time from the side panel | A separate dusk orders screen; one duty per squad for 24 hours; watches of a few hours | Night guard is planned on its own without a second stop each day, which would interrupt the skip-ahead and fast play. One 24-hour duty leaves guards watching day and night; short watches mean too many clicks each morning, better left to a later standing instruction |
| 42 | Sending a squad that is on duty somewhere else | It leaves its duty and stays where it was sent, counting as awake (so it tires). At the next dusk or dawn it takes up its rostered duty again by itself; a "Back to duty" button sends it back sooner | Going back by itself a set time after arriving; staying until returned by hand | The player's order always wins, and no post is left unmanned for long by accident |
| 43 | The roster's details (director approved the plan as a whole) | Tiredness builds while a man is awake and falls while he sleeps in a shelter on rest duty: about 18 hours awake to exhausted, 8 hours' sleep from exhausted to fresh; an exhausted man spots at half the distance and walks at 70% of his speed, a tired one in between. Six lookout posts on the half-circle of trench about 60 m out from the top, one squad each; three shelters behind the crest: the great cavern (10 squads), the large hut (3), the old counter-mine gallery (2). The first morning's roster splits the company in three: 5 squads guard by day, 5 by night, 5 rest in reserve. The Italians do not tire until the enemy officer arrives. Squads walking to their roster places raise no "arrived" messages. Step-2 replays saved before the roster are refused; step-1 replays still play | Per-watch rotation inside a squad (the sources say sentries changed every two hours; left to a later standing instruction); a reserve shelter at "Alpenrose" (the reserve companies there are outside the player's company) | The plan as presented to the director. The numbers make a squad that guards by day tired but not exhausted by dusk, exhausted if it also guards the night, and fresh again after a night's sleep |

### Sources for the summit position (decisions 40 and 43)

The sources describe the position but give no exact spots; every place's spot was read from the
terrain by Claude (open view to the south for posts, out of sight of the southern slopes for
shelters), and the data file says so.

- [frontedolomitico.it: winter preparations, December 1915 to January 1916](https://www.frontedolomitico.it/Luoghi/coldilana/19_inverno.html)
  — several lines of trench in a half-circle around the two summit humps, about 200 m long; forward
  listening posts of four or more men; a double belt of wire; reserve companies at "Alpenrose"
  relieving the front every 24 hours
- [frontedolomitico.it: the Austrian wait, April 1916](https://www.frontedolomitico.it/Luoghi/coldilana/24_attesa.html)
  — the great cavern where about 100 men crowded; the large hut, the officers' hut, the kitchen and
  listening caverns; sentries changed every two hours; the garrison of 17 April, 9 officers and 238
  men, the Kaiserjäger company 6 officers and 140 men; two platoons in the forward positions
- [frontedolomitico.it: tunnel works, January to March 1916](https://www.frontedolomitico.it/Luoghi/coldilana/22_lavori.html)
  — the Austrian counter-gallery, stopped at 25 m and made into a troop shelter
- [frontedolomitico.it: the explosion](https://www.frontedolomitico.it/Luoghi/coldilana/26_esplosione.html)
  — part of the cavern blown up; consistent with placing it just north of the top, by the crater
- [Austria-Forum: Col di Lana](https://austria-forum.org/af/AustriaWiki/Col_di_Lana) — literature
  (Tschurtschenthaler, Schemfil), not consulted directly

**Found on the way, for the night raid:** in early 1916 the Italian front trench lay only tens of
metres below the summit (their mine gallery was about 50 m long, its head halfway between the two
wire belts), while the game's Italian patrols stay more than 1 km away.
