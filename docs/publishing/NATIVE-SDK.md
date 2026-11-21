# ViaHorizon native SDK package

The native `.hmod` uses the prebuilt game and mod SDK for the exact commit in
[`native/horizon-mod.toml`](../../native/horizon-mod.toml). The launcher compiles
the mod and its function replacements; it does not compile the game.

## Build and verify

Use Python 3.11+, the Horizon Launcher, and `cargo-horizon-mod` built from the
pinned Horizon source. From this repository, with a pristine checkout of that
commit:

```powershell
python tools/native_hmod.py --horizon PATH_TO_HORIZON
Push-Location target/native-hmod
cargo horizon-mod build
cargo horizon-mod check target/horizon-mod/viahorizon.hmod --kind native-mod --json
Pop-Location
```

The source under `native/` is a template. `native_hmod.py` combines it with the
shared protocol sources in `mixins/viahorizon/` and the pinned Horizon codecs.
It writes a flat package project in `target/native-hmod/`; do not package the
template or edit the generated files by hand.

The package check is not a runtime test. Install the pinned Nightly and
prepare its resources in a disposable launcher instance. Build the staged
project in that instance, then join vanilla servers on at least one explicit
legacy target and one auto-detected target. Include a vanilla 26.3 connection
and a single-player world to check that native connections are unaffected.

```powershell
horizon-launcher native-build target/native-hmod --instance "ViaHorizon Test" --trust-native-code
```

The Windows `tools/vanilla-gameplay.py` harness can test an isolated copy of the
result, including the dynamic libraries and `native-mods.json`, with
`--client`, `--ctl` and `--resources`. Allow enough time for a cold resource
load (e.g. `--timeout 420`). The harness creates only local, disposable worlds.

The previous `.hnmod` builds and tests remain available through
[`tools/native.py`](../../tools/native.py). Do not replace already published
assets; publish a new version from the corresponding source commit.
