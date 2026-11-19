# Alpine Front — instructions for Claude Code

A First World War Alpine-front strategy game in Rust and Bevy, in which every soldier is simulated
as an individual. Claude Code writes all the code; the human is director and designer and does not
review code.

## Source of truth

- `docs/design-brief.md` is the design. Re-read it at the start of each session. Change it first,
  then the code.
- `docs/decisions.md` records why each decision was taken and what was rejected. Add an entry
  whenever a decision changes.
- Everything the project needs lives in this repository. Do not depend on outside documents.

## Working rules

- Correctness is judged by behaviour, so replays, debug overlays and scenario tests are core work,
  not extras.
- Scenario-test expectations: Claude drafts them, the director approves them, then they are locked.
  Never change an approved expectation without asking. Tuning numbers live in readable data files,
  and every test run ends in a plain-language report.
- Write lasting tests only for things not expected to change (decision 56): the basic rules, not
  this week's numbers or behaviour a later step will replace. Those are checked by hand while they
  are being built, and judged by playing.
- Every game rule can be switched off on its own (decision 65). Each rule, and each optional part
  of a rule (for example breaking trail in fresh snow, or snowshoes), gets an on/off switch in its
  readable data file. Switched off, the game behaves as if that rule had never been built. Keep
  rules apart in the code so that switching one off never needs code changes or breaks another.
- The simulation is plain Rust with a fixed run order and seeded randomness, and has no Bevy or
  rendering dependency. Bevy only draws a snapshot. Every test runs twice and the results must match
  exactly.
- After every build step, run the speed pass described in the brief's Build order section (8 of 16
  threads pinned two to a core on 4 cores; the simulation work in each frame uses at most half the
  frame's time).
- Follow the build order. Nothing starts until the step before it is playable. Step 2 is
  deliberately crude.
- Explain decisions to the director in plain language, with options, pros and cons, and a
  recommendation.
