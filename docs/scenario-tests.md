# Scenario tests

Design statements written as automated tests. Claude drafts them, the director approves them, and
from then on they are **locked**: Claude may not change an approved expectation without asking.
Tuning numbers the tests depend on (spotting range, the steepest walkable slope, walking speeds) live
in [data/tuning.toml](../data/tuning.toml); the tests read them from there.

Run them with `cargo test --release -p alpine_sim --test scenarios`. Every scenario runs twice and
both runs must match exactly; the run ends in a plain-language report.

Places are given in metres from the Col di Lana summit (east, north).

## Step 1: pipeline (approved 2026-11-17)

| # | Expectation |
| --- | --- |
| 1 | **Exact replays.** The same seed and orders give an identical result, compared every simulation tick, run twice. A saved replay played back matches the original exactly. Running on 1 thread or 8 gives the same result. |
| 2 | **Rounded summit.** A soldier 250 m down the south face (0, −250) is out of sight of a soldier on the summit (0, 0), and the other way round. |
| 3 | **Ridge.** Soldiers 250 m down the south face (0, −250) and 250 m down the north face (0, 250) cannot see each other. |
| 4 | **Summit and Monte Sief.** The summit and Monte Sief's top (−400, 460), 610 m apart, can see each other, and an enemy standing on either one is spotted from the other within 2 game seconds. |
| 5 | **Too far to spot.** Monte Sief's top (−400, 460) and Settsass's top (−140, 2480), 2 km apart, have a clear line of sight, but a soldier on one is not spotted from the other, because 2 km is beyond the 1,500 m spotting range. |
| 6 | **No impossible ground.** Nobody ever stands on ground steeper than 45°, in any test. |
| 7 | **Cliff detour.** A squad sent from the meadow 900 m north of the summit (0, 900) to the summit, where the straight line crosses a 59° cliff, goes around it and arrives within 2 game hours. |
| 8 | **Long climb.** A squad climbing from the valley floor south-west of the summit (−1200, −1600; 1,217 m) to the top (2,460 m) takes 3 to 6 game hours. A standard Alpine trail-sign walking-time rule (DIN 33466) gives about 4.4 hours. |
| 9 | **Descent.** Coming down the same way takes 40–80% of the climb time. |
| 10 | **Nobody gets stuck.** In a 2-hour run with every squad on random orders, no soldier who has not arrived stands still for more than 2 game minutes. |
| 11 | **Spacing.** Once arrived, the men of a squad stand at least 1 m apart. |

**Measured on today's terrain.** Tests 2–5 and 7–9 depend on the exact shape of the ground in
`data/terrain/col_di_lana.heights` (TINITALY 1.1, which shows the summit after the April 1916 mine
took roughly 27 m off it). When the pre-blast summit is rebuilt, Claude re-measures these spots and
brings the affected tests back to the director for re-approval before changing anything (decided
2026-11-17).

Director's notes on approval: soldiers will later be able to climb steeper ground given equipment,
skill and suitable conditions, or by building via ferratas, so the 45° limit is a step-1
simplification. Line of sight and the spotting range here assume good weather.
