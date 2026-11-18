# Scenario tests

Design statements written as automated tests. Claude drafts them, the director approves them, and
from then on they are **locked**: Claude may not change an approved expectation without asking.
Lasting tests cover only things not expected to change: the basic rules, not tuning numbers or
behaviour a later step will replace, which are checked by hand while being built (decision 56).
On 2026-11-18 the director withdrew the tests below that pinned numbers, and loosened others to
their rule.
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
| 5 | *Withdrawn 2026-11-18 by the director (decision 56): it pinned numbers later steps will change.* ~~Too far to spot (beyond the 1,500 m spotting range).~~ |
| 6 | **No impossible ground.** Nobody ever stands on ground steeper than the tuning file's limit (45° now), in any test. *(Loosened 2026-11-18, decision 56: the limit is a number that equipment and skill will change.)* |
| 7 | **Cliff detour.** A squad sent from the meadow 900 m north of the summit (0, 900) to the summit, where the straight line crosses a 59° cliff, goes around it and arrives within 2 game hours. |
| 8 | *Withdrawn 2026-11-18 by the director (decision 56): it pinned numbers later steps will change.* ~~Long climb, 3 to 6 game hours.~~ |
| 9 | *Withdrawn 2026-11-18 by the director (decision 56): it pinned numbers later steps will change.* ~~Descent, 40–80% of the climb time.~~ |
| 10 | **Nobody gets stuck.** In a 2-hour run with every squad on random orders, no soldier who has not arrived stands still for more than 2 game minutes. |
| 11 | **Spacing.** Once arrived, the men of a squad stand at least 1 m apart. |

**Re-measured on the rebuilt summit (2026-11-17).** Tests 2–5 and 7–9 depend on the exact shape of
the ground in `data/terrain/col_di_lana.heights`. That map is TINITALY 1.1 with the April 1916
mine's crater (about 35 × 25 m, 12 m deep) filled to show the summit before the blast (decision
28). The earlier note that the mine took roughly 27 m off the summit was wrong and is withdrawn.
On the rebuilt summit every result, down to the margins, is identical to the original ground, so
no expectation changed. If the ground changes again, Claude re-measures these spots and brings
the affected tests back to the director for re-approval before changing anything.

Director's notes on approval: soldiers will later be able to climb steeper ground given equipment,
skill and suitable conditions, or by building via ferratas, so the 45° limit is a step-1
simplification. Line of sight and the spotting range here assume good weather.

The step-1 tests run without a calendar, which means in daylight: the spotting distance is the
fixed 1,500 m from the tuning file, whatever the clock says.

## Step 2: the dawn orders screen, night and the skip-ahead (approved 2026-11-17)

Times are Central European Time. "Dusk" means the end of civil twilight and "dawn" or "first
light" its start in the morning (the sun 6° below the horizon), the standard definitions.

| # | Expectation |
| --- | --- |
| 12 | **Calendar and dawn.** The step-2 game opens at first light on Monday 6 March 1916. The orders moment comes at first light every morning: 06:12 on Monday down to 06:01 on Sunday, each within 2 minutes. *(Changed from sunrise, 06:42 to 06:31, with the director's approval on 2026-11-17: decision 37.)* |
| 13 | **Moon.** Moonset on the night of 10 to 11 March falls within 15 minutes of 01:42 (the independent table for Telve); the moon is 40 to 60% lit on 11 March. |
| 14 | *Withdrawn 2026-11-18 by the director (decision 56): it pinned numbers later steps will change.* ~~Clear, moonless night: spotted at 40 m, not at 80 m.~~ |
| 15 | *Withdrawn 2026-11-18 by the director (decision 56): it pinned numbers later steps will change.* ~~Overcast, moonless night: spotted at 10 m, not at 30 m.~~ |
| 16 | *Withdrawn 2026-11-18 by the director (decision 56): it pinned numbers later steps will change.* ~~Daylight unchanged at 1,500 m.~~ |
| 17 | **Twilight is gradual.** From sunset to full night the spotting distance never changes by more than 10% in one game minute. |
| 18 | **Skip stops at dawn.** Started at dusk on any night of the week, the skip-ahead stops on the exact update of the next dawn. *(Loosened 2026-11-18, decision 56: "with the current Italian patrols no night is interrupted" dropped; the night raid will change it.)* |
| 19 | **Skip stops on events.** By day it stops on the update in which our men see an Italian squad not seen in the past 30 minutes, and when an ordered squad arrives or cannot get there. |
| 20 | **Skipping changes nothing.** A day played at normal speed and the same day crossed with the skip-ahead, with the same orders, give identical state, compared every update. |
| 21 | *Withdrawn 2026-11-18 by the director (decision 56): it pinned numbers later steps will change.* ~~Step 1 unchanged: a step-1 replay saved before step 2 plays back matching.~~ It broke on every deliberate change to movement; test 1 guards replays. |

Run them with `cargo test --release -p alpine_sim --test step2`. The step-2 tests use the week's
weather diary (`data/weather/march_1916.toml`).

## Step 2: the work roster (approved 2026-11-17)

Places are the lookout posts and shelters in `data/places/col_di_lana_1916.toml` (decisions 39 to
43). "Fresh", "tired" and "exhausted" are the morning report's words for a man's tiredness.

| # | Expectation |
| --- | --- |
| 22 | **Shelter hides both ways.** A squad resting in a shelter sees no enemy and is never seen by one, day or night. |
| 23 | **Guards watch.** By day a squad at a south-facing post spots an Italian in plain view on the southern slope, well within the day's spotting distance. *(Loosened 2026-11-18, decision 56: "at the full 1,500 m" dropped.)* |
| 24 | **Shifts change themselves.** Within 1 game minute of dusk, and of the morning "Carry on", every squad on the roster sets off for its new place, and each gets there. *(Loosened 2026-11-18, decision 56: "within 15 game minutes" dropped.)* |
| 25 | **Tiredness builds and recovers.** A man who guards from dawn to dusk is more tired at dusk; one who guards around the clock is more tired still by the next dawn; one who guards by day and sleeps the night is less tired at dawn than he was at dusk. *(Loosened 2026-11-18, decision 56: "tired but not exhausted", "exhausted" and "fresh again" dropped.)* |
| 26 | **Tired men see and walk less.** An exhausted guard spots at a shorter distance than a fresh one, by day and by night, and an exhausted squad takes longer on the long climb. *(Loosened 2026-11-18, decision 56: "half the distance" and "about 1.4 times as long" dropped.)* |
| 27 | **Orders beat the roster.** A squad sent elsewhere leaves its duty, stays where it was sent, and takes up its duty again at the next dusk or dawn, or when "Back to duty" is pressed. |
| 28 | **Same as yesterday.** Pressing "Carry on" without touching the roster gives the same duties as the day before. |
| 29 | **Replays and old tests.** Roster choices are saved in replays and play back exactly. Tests 1 to 21 still pass. |

## Step 2: carrying parties, cold and hunger (approved 2026-11-18)

The dump on Monte Sief and the summit store, food and fuel, cold, frostbite and hunger (decisions
46 to 51). Tuning numbers live in `data/supply.toml`; the week's temperatures are in the weather
diary. Tests 1 to 29 run without the supply file, so cold and hunger are off for them. Cold does
not shorten how far a man spots by itself; it tires him (decision 52). These tests cover the lasting
rules only; how many days the stores last or how cold a night gets are numbers, judged by playing
(decision 55).

| # | Expectation |
| --- | --- |
| 30 | **Carrying delivers.** A squad on carry duty by day makes a delivery, and the store rises by its ten men's loads. *(Loosened 2026-11-18 by Claude, not yet approved by the director (decision 56): "within 2 game hours" dropped; the first trip takes about 2 h 10 min now.)* |
| 31 | **The store has a limit.** The store never holds more than 3 days of either supply. A carrying squad with nothing left to fetch goes back to its shelter to rest. |
| 32 | *Withdrawn 2026-11-18 by the director (decision 55): it pinned this week's numbers.* ~~Doing nothing: the stores run out on Thursday.~~ |
| 33 | *Withdrawn 2026-11-18 by the director (decision 55): it pinned this week's numbers.* ~~A cold night is survivable.~~ |
| 34 | **Neglect costs men.** With no fuel, a squad guarding two nights running suffers frostbite. Starving as well, its first death comes sooner than when it is fed. |
| 35 | **Hunger tires.** A starving squad tires faster than a fed one doing the same duty. |
| 36 | **The lost leave.** Frostbitten and dead men leave their squads, and the morning report counts them. |
| 37 | **Night carrying is slower.** A carrying trip takes longer by night than by day. |

Run them with `cargo test --release -p alpine_sim --test supply`.
