# Alpine Front

A First World War strategy and tactics game set on the Alpine front. Every soldier is simulated as
an individual, and the mountain itself (altitude, snow, rock, cold) is the main opponent. The player
commands an Austro-Hungarian company holding the summit of Col di Lana through a season.

**Status:** build step 1 (pipeline) in progress. The simulation core and its locked scenario tests
are in place; the 3D view is next.

- [Design brief](docs/design-brief.md): what the game is, the agreed decisions, architecture and
  build order
- [Decision log](docs/decisions.md): why each decision was taken

- [Scenario tests](docs/scenario-tests.md): the locked design statements the tests check

Built in Rust with the Bevy engine.

## Running the tests

    cargo test --release -p alpine_sim

The scenario tests end in a plain-language report.

## Elevation data

The terrain in `data/terrain` is cut from TINITALY by `tools/prepare_terrain.py` and is used under
the CC BY 4.0 licence:

Tarquini S., I. Isola, M. Favalli, A. Battistini, G. Dotta (2023). TINITALY, a digital elevation
model of Italy with a 10 meters cell size (Version 1.1). Istituto Nazionale di Geofisica e
Vulcanologia (INGV). https://doi.org/10.13127/tinitaly/1.1

No license has been chosen yet, so all rights are reserved for now.
