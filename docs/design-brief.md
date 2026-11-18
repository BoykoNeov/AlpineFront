# Alpine Front — Game Design Brief (Stage 1)

This file is the source of truth for the design. Decisions are changed here first, then in code.
Why each decision was taken, and what was rejected, is in [decisions.md](decisions.md).

## Vision

A First World War strategy and tactics game set on the Alpine front, in which every soldier is
simulated as an individual. The player commands an Austro-Hungarian company holding the summit of
Col di Lana through a season or a year. Battles are episodes inside a longer game of survival,
engineering and supply.

The target is 3,000 to 5,000 simulated soldiers at once, with several hundred as the fallback. The
first playable version needs only 150 to 250.

The game should make the landscape itself the main opponent: altitude, snow, rock, cold and the
difficulty of moving anything uphill.

## Roles and workflow

Claude Code writes all the code. The human is director and consulting designer and does not review
code, so correctness is judged by behaviour.

That makes verification tooling part of the first milestone, not a later extra:

- **Replays.** Any run can be reproduced exactly from a seed and an order log. This holds on the
  same machine and game version; every test runs twice and must match. Saved games are full
  snapshots, because code changes break old replays.
- **Debug overlays.** Morale, supply, line of sight and belief state can be shown on the map.
- **Scenario tests.** Design statements written as automated tests, for example "a platoon
  attacking uphill over open scree against one machine gun fails nine times in ten." Claude drafts
  the expected outcomes; the director approves them, and they are then locked: Claude may not
  change one without asking. Tuning numbers live in readable data files, and each run ends in a
  plain-language report.
- **Speed pass.** After every build step, on the development PC (the only test machine). See
  [Build order](#build-order).
- **This document.** Claude Code re-reads it each session; decisions are changed here first, then
  in code.

## Agreed decisions

| Area | Decision | Reason |
| --- | --- | --- |
| Language | Rust | Performance, and the strict compiler acts as a first reviewer for code no human reads |
| Engine | Bevy, pinned to one version | Rust-native, everything is code with no visual editor; used for drawing and input only, the simulation does not depend on it |
| Simulation | Headless, deterministic core in its own crate with no rendering dependency | Testable and replayable without a screen; the view is replaceable |
| Simulation base | Plain Rust with its own data and a fixed run order, splitting work across cores explicitly; Bevy only draws a snapshot each frame | Exact replays by design, and Bevy upgrades cannot change how the simulation behaves |
| Terrain | 3D, built from real elevation data | Conveys the landscape of mountain warfare |
| Units | Simple shapes when zoomed out, low-detail figures built in code with a few poses up close; instanced | Cheap to draw in thousands; no artist needed |
| Views | Two views of equal standing, switched with a key: the 3D terrain and a 2D map drawn as a First World War staff map (paper, hand-drawn style relief, lettered names, squad symbols when zoomed out and men up close). Everything the player can do works in both | The map reads where 3D hides men behind ridges, suits how the war was commanded, and is the natural place for "the map shows reports, not reality" and for the sector seat |
| Screens | egui for now; the final period look of the screens is decided before the campaign step, and the map's look is revisited then | Fast to build now, and no styling wasted on screens that may change |
| Target hardware | Strong gaming PC as the minimum: 8 cores, 32 GB memory, RTX 3070 class graphics | Headroom for soldier count and terrain; a speed pass after every build step keeps it honest |
| Player seat, first | Company commander on one position, 150 to 250 men | Exercises every mechanic on a small map |
| Player seat, later | Sector commander with several battalions | Same simulation, player moved up the chain |
| Time | Continuous, with pause and speeds up to 64×, plus a skip-ahead that runs as fast as the computer can until dawn or until something happens; events show a message, and slow normal play only if the player chooses (decision 38) | A company's life runs from seconds to weeks: 64× lets a quiet night pass in about 10 minutes while still watchable, and the skip-ahead jumps the dead hours |
| Core loop | Continuous time plus a daily orders moment at dawn (optionally dusk): work roster, night patrols, standing instructions, requests to headquarters, with "same as yesterday" defaults. Building projects run for days in the background. The game stops by itself at first light (dawn: the sun 6° below the horizon, decision 37) and opens the orders screen with a morning report; each part of the screen arrives with the mechanic it orders (decision 32) | Gives quiet weeks real choices and matches how a company was run |
| Light | Calendar time drives the light: the sun's height (with twilight), the moon's phase and height, starlight, and cloud. The light sets how far a man can be spotted, from 1,500 m by day to about 50 m on a clear moonless night and 15 m under overcast (decisions 33 to 35) | Sightings at night must be believable, in the reports and on the map |
| Campaign | One company, one position, a season or a full year | Combat sits inside survival, engineering and supply |
| Scenarios | Standalone set pieces, historical or invented | For example the mining of the Col di Lana summit |
| Setting | Col di Lana. Modern elevation data shows the crater of the April 1916 mine (about 35 × 25 m and 12 m deep), so the game fills it to show the summit as it was before the blast | Its rounded shape suits the heightmap; a season of attacks ends in the mine; Monte Sief along the ridge leaves room to grow. Lagazuoi's ledges and overhangs need a far more complex terrain model |
| Sides | Austria-Hungary first, as the summit defenders; Italy second, built around digging the mine | Defenders live on the mountain all season, so survival, building and the mine dread are the core; one side limits early content work |
| Soldier model | Medium, built in layers: body (wounds by location, tiredness, cold and frostbite, hunger, illness), mind (morale, shock, one or two traits), abilities (shooting, mountain craft, digging, languages spoken), kit, identity, ties (home village, one close comrade), a short history log. Each build step adds its own part | Real stories without an untunable model. Rule: nothing goes in unless it changes what happens and the player can see it |

## Architecture

The simulation and the presentation are separate programs in one workspace. The simulation never
asks the renderer for anything.

**Simulation core**

- Plain Rust data, independent of Bevy; each soldier's state follows the soldier model in Agreed
  decisions.
- Fixed time step, seeded randomness, and an order log, so every run is reproducible.
- Runs headless for tests and for fast-forwarding quiet periods.

**Terrain as layers, not voxels**

- A deformable heightmap for the ground, edited by craters and mine explosions.
- A snow-depth layer on top, which changes movement, concealment and avalanche risk.
- A separate tunnel and cavern graph underground, shown through a cutaway view.
- Known limit: a heightmap cannot represent overhangs or cave mouths in cliffs.

**Fields on coarse grids**

- Wind, temperature and precipitation.
- Gas, which flows downhill and pools in low ground, driven by the wind field.
- Sound, for detecting enemy digging.

**Performance approach**

- Squads decide and individuals execute.
- Flow-field pathfinding: each squad gets its own route, worked out only over the ground around its
  men and their way to the destination. Sharing routes between squads is added only if large forces
  show the need.
- A spatial grid for neighbour and line-of-sight queries.
- Staggered updates: not every soldier thinks every tick.
- Slow systems such as disease, food and morale drift tick once per game-hour.

**Presentation**

- Chunked terrain mesh with level of detail, a free camera, and instanced unit shapes.
- A contour overlay and a "what can this observer see" shading mode, because ridges hide units from
  the camera.
- A 2D map view, drawn separately as a period staff map rather than as the 3D world seen from above,
  so it can later show things differently (symbols, names, reports instead of truth). Rule: any
  feature the player uses on the ground (selecting, orders, overlays, and later targeting, building
  and digging) is not done until it works in both views. Clicking, orders and the side panels are
  one piece of code shared by both views; only the drawing is separate. Own troops are blue and the
  enemy red in both views.

## Command and communications

The simulation keeps ground truth separate from what each commander believes. This is built in from
the start because it is very hard to add later.

- **Generic hierarchy.** Soldier, squad, platoon, company, battalion, sector. The player can occupy
  any officer's seat; orders come from above and go below.
- **One officer AI.** The same system runs enemy officers, the player's subordinates and cut-off
  units.
- **Orders and reports are physical.** They travel by runner, telephone wire, signal lamp or pigeon,
  each with its own delay and failure chance. Lamps need line of sight and clear weather; shelling
  cuts wire.
- **The map shows reports, not reality.** Unit markers age and fade until a new report arrives.
- **Cut-off units act on three inputs:** their last orders, standing instructions set beforehand
  ("if isolated, hold" or "fall back to the saddle"), and the officer's traits such as initiative,
  caution and competence. Officers have bounded initiative: within limits set by their traits they
  may pull back, counter-attack or send for help, and they report what they did and why when contact
  returns. Standing instructions set how loose the leash is.
- **Succession.** When an officer is killed, the next in rank takes over with his own traits.
- **Belief is tracked per officer, not per soldier.**

Realism is a setting, not a design fork. The simulation always runs the full model; the setting
controls how much truth the interface shows.

| Level | What the player gets |
| --- | --- |
| 1 | True positions and instant orders |
| 2 | True positions, delayed orders |
| 3 | Delayed orders and delayed, uncertain reports |
| 4 | The player is at a headquarters on the map and can be cut off |

The default is level 3. Level 4 is offered as the historical setting; revisit once the rough game is
playable.

## Mechanics in scope

All of these are intended; none is designed in detail yet.

| Mechanic | Design note so far |
| --- | --- |
| Terrain | Slope, rock and cover drive movement, line of sight and fire |
| Weather | Rain, snow, fog, wind and cold affect visibility, movement, signalling and health. Cloud already dims the night from step 2 |
| Snow cover | A depth layer that accumulates and melts |
| Avalanches | Both hazard and weapon, tied to snow load, slope and shelling |
| Artillery | True ballistic arcs against the heightmap, so dead ground and gun type matter |
| Rock splinters | Shells on rock cause more casualties than on soft ground |
| Poison gas | Flows downhill, pools in valleys, moved by wind |
| Terrain change | Craters from shells; large reshaping from underground mines |
| Tunnels and mines | Digging takes weeks; listening posts hear enemy work; counter-mine or evacuate |
| Close combat | Raids, trench and cavern fighting |
| Wounds | Individual wound state, evacuation and recovery |
| Disease | Slow-ticking, linked to cold, food and shelter |
| Food and logistics | Cableways, mule tracks and carrying parties; supply can be cut |
| Morale | Per soldier, shaped by losses, cold, hunger, leadership and isolation |
| Communications | Telephone wire, runners, lamps, pigeons; see the section above |
| Individual identity | Names, home villages and histories, for the stories the simulation produces |
| Building | Huts, caverns, cableways, wire belts and counter-mine tunnels as multi-day projects, set at the daily orders moment; the main activity of quiet weeks |

## Build order

Each step ends in something the director can run and judge.

After every step comes a speed pass on the development PC, the only test machine. With the
simulation limited to 8 of the PC's 16 threads, placed two to a core on 4 cores, the simulation
updates run during each frame must use no more than half that frame's time (decision 30), and each
frame must finish well inside a limit sized to leave
room for an RTX 3070 class card. These margins are estimates, not measurements on real target
hardware.

- **Simulation check.** Measured with the simulation running in real time at the fastest game speed
  the player can select, which was 16× in step 1 and is 64× from step 2 (decision 29). It is
  judged per frame at 60 frames per second: the updates falling in each 1/60 s of real time must
  take no more than 8.3 ms together. Single slow updates are reported but do not fail on their own
  (decision 30). The skip-ahead, which runs as fast as the computer can, is not held to a limit. The report states how
  much faster than real time the simulation could run, which is how fast the skip-ahead goes.
  Since the work roster there are two runs, and both must pass: the heaviest load (every squad
  moving on random orders at noon) and the game as played (the roster across Monday's dusk, when
  every squad changes over at once). The frame check plays the roster game (decision 44).
- **Frame check.** 30 frames per second on an RTX 3070. The development PC's RTX 5090 is taken as
  about three times faster (an estimate from public benchmarks), so 99% of frames must finish within
  11 ms here, measured at 2560×1440 with vsync off while the camera flies over the terrain. The
  report also gives the processor's and graphics card's work per frame. If the graphics driver holds
  frames back beyond the limit (for example when the window is not in front), the check reports "not
  measured" instead of passing or failing.

1. **Pipeline.** Col di Lana terrain on screen, 200 soldiers (as simple shapes) moving across it
   with correct line of sight, a free camera, the debug overlays, and a replay from a seed. No
   combat.
   - Map: 6 × 6 km centred on the summit, from the TINITALY 10 m elevation model. It shows today's
     summit, crater included; the pre-1916 summit is rebuilt at the start of step 2 (done: the
     crater is filled, decision 28).
   - About 150 Austro-Hungarians on the summit and 50 Italians on the slopes below, in squads of
     about 10. The player clicks a squad and a destination; Italian squads follow seeded patrol
     routes.
   - Line of sight decides who has spotted whom, and the map shows it. There is no shooting.
   - Overlays: contour lines, "what this observer can see" shading, spotting lines, squad routes,
     and ground too steep to walk.
   - Added after step 1 was finished, before step 2 starts: the 2D period map (see Presentation),
     with every control and overlay of the 3D view. From here on the speed pass measures frames in
     both views, and both must pass.
2. **A week on the summit, rough.** First, the summit is rebuilt as it was before the April 1916
   mine (from old photos and maps), and the summit sight tests are re-measured and re-approved
   (done 2026-11-17: the mine's crater is filled and every test result is unchanged, decision 28).
   Then the dawn orders screen, work roster, carrying parties, simple
   cold and hunger, falling snow, and one night raid with basic shooting and a simple enemy officer.
   The whole game in miniature; crude is fine and is not polished.
   - The dawn orders screen brings the calendar clock, the skip-ahead and the light model with it.
     For now the screen holds the date, the weather and a morning report of the last 24 hours, and
     a "Carry on" button (decision 32). The skip-ahead stops at dawn, when our men see an enemy
     squad not seen in the last 30 minutes, and when a squad we ordered arrives or cannot get there
     (decision 36). In normal play the same events show a message; the player chooses whether
     they also slow the game, to 8× or to 1×, and by default they do not (decision 38).
   - The light model and the week's cloud come forward from step 7 (decisions 33 and 34): the sun,
     the moon and cloud set the spotting distance. The cloud comes from a weather diary for the
     week, an invented but plausible late-winter week in a readable data file, which the falling
     snow piece will extend.
   - The work roster (decisions 39 to 43). At the dawn orders screen each squad gets a day duty
     and a night duty, guard or rest, with "same as yesterday" filled in; squads change over by
     themselves at dusk and dawn, and the roster can be changed at any time from the side panel.
     Guards watch from lookout posts; resting men sleep in shelters, where they neither see nor can
     be seen. The places follow period sources: about 200 m of trench in a half-circle around the
     two summit humps with forward posts, and behind the crest the great cavern (about 100 men),
     the large hut and the old counter-mine gallery. Exact spots are read from the terrain, since
     no source gives them; they live in `data/places/col_di_lana_1916.toml`. Men tire while awake
     and recover asleep: about 18 hours awake leaves a man exhausted, 8 hours' sleep makes him
     fresh, and an exhausted man spots at half the distance and walks at 70% of his speed. A squad
     sent elsewhere leaves its duty until the next dusk or dawn, or until "Back to duty". The
     Italians do not tire yet; that comes with the enemy officer. Carrying is added with the
     carrying parties; digging waits for building (step 6).
   - Carrying parties and simple cold and hunger are built as one piece (decision 45): the summit's
     stocks run down each day, men go hungry and cold when they run short, and squads on carrying
     duty bring supplies up, at the cost of not guarding or sleeping. Its details are planned with
     the director before it is built.
   - The week is Monday 6 to Sunday 12 March 1916 (decision 31): deep snow and hard cold, the
     Italians tunnelling toward the summit since mid-January. On the summit the sun rises at about
     06:40 and sets at about 18:10 (Central European Time, before summer time began that April), so
     nights last about 12½ hours. First light, when the orders moment comes, is about half an hour
     before sunrise: 06:12 on Monday, 06:01 on Sunday.
3. **Fire and morale.** Small-arms combat, suppression, cover, and soldiers who break.
4. **Officer AI.** Enemy and subordinate officers acting on orders, traits and partial knowledge.
5. **Artillery and craters.** Ballistic fire, observers, terrain deformation.
6. **Supply and building.** Food, ammunition, carrying parties, cableways, huts, caverns, wire.
7. **Weather and snow.** Fields, snow layer, cold, avalanches. (Light, night and the step-2
   weather diary already exist from step 2; this step replaces the diary with weather fields.)
8. **Tunnels and mines.** Digging, listening, counter-mining, the summit explosion.
9. **Campaign and scenarios.** The season-long game and standalone set pieces. The final screen look
   is decided before this step.
10. **Italy and the sector seat.** Italy becomes playable, built around digging the mine; then the
    player moves up the chain, with the full unit count and a larger map.

Design topics to settle before their step, in this order: fire and morale, officer AI, supply and
weather.

## Risks and open questions

**Risks**

- **Scope.** The mechanics list is long. The build order is the defence: nothing starts until the
  step before it is playable.
- **Officer AI quality.** A subordinate who wastes his men through weak AI reads as a bug. The
  sector seat depends on this being good. A crude version arrives in step 2 so problems show early.
- **Engine churn.** Bevy's interface changes between releases. Pin one version and work from that
  version's documentation.
- **Readability in 3D.** Terrain hides units. The overlays in the architecture section must arrive
  with the first milestone.
- **Two interfaces.** Company and sector seats need different screens, which roughly doubles
  interface work.
- **Two views.** Everything drawn on the ground (men, lines, shading, and later craters, wire and
  tunnels) is drawn twice, once in 3D and once on the map. Sharing clicks, orders and panels keeps
  this to the drawing alone; watch that it stays so.
- **Scale.** Thousands of soldiers on a large map is unproven until the sector step; measure
  performance from the first milestone.

**Open questions**

- [x] What did the Col di Lana summit look like before the April 1916 mine? Answered 2026-11-17
      (decision 28): much as today, without the mine's crater, which is about 35 × 25 m and 12 m
      deep. No height from before the war has been found; if one turns up, the rebuild script
      takes it as a single number.
- [ ] Does Veneto's laser-scan data cover Col di Lana in finer detail than its 10 m terrain grid?
