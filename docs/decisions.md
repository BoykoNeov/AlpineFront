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

### Sources used

- [Col di Lana (Wikipedia)](https://en.wikipedia.org/wiki/Col_di_Lana)
- [Mines on the Italian front, World War I (Wikipedia)](https://en.wikipedia.org/wiki/Mines_on_the_Italian_front_(World_War_I))
  — the Italian mine of 17 April 1916 (about 5 tonnes) and the earlier Austrian counter-mine of 6 April 1916
- [Smithsonian: Climbing the Via Ferrata](https://www.smithsonianmag.com/travel/climbing-the-via-ferrata-8081616/)
  — summit roughly 90 feet lower after the blast
- [Lagazuoi open-air museum](https://lagazuoi.it/Eng/page7-Open-air-Museum-of-the-Lagazuoi)
- [Veneto terrain model (10 m mountain grid)](https://www.regione.veneto.it/web/agricoltura-e-foreste/modello-digitale-del-terreno)
- [Veneto laser-scan data](https://www.regione.veneto.it/web/agricoltura-e-foreste/modelli-digitali-delle-chiome)
