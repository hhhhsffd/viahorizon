# ViaHorizon

Choose the Minecraft server version when playing with Horizon 26.3.
ViaHorizon translates Release protocols from 1.7.2 through 26.3.

## Install

Download `viahorizon-0.9.0-dev.hmod` from
[Releases](https://github.com/hhhhsffd/viahorizon/releases). It is built for
the **November 20, 2026 Nightly (`0a875da`)**. Install and prepare that Nightly
in Horizon Launcher first. The launcher supplies the native build tools.

```powershell
python -m zipfile -e .\viahorizon-0.9.0-dev.hmod .\viahorizon-0.9.0-dev
horizon-launcher native-build .\viahorizon-0.9.0-dev --version nightly-2026-11-20-0a875da --trust-native-code
```

Native mods run with your account's permissions. Choose the server version
under **Options → Mods → ViaHorizon → Config**. Leave it on **Use config.json**
to use existing settings or auto detection on a fresh install.

## Build

See the [native SDK build and validation guide](docs/publishing/NATIVE-SDK.md).
Run `cargo test --locked` to exercise the shared protocol code. For the old
`.hnmod` workflow, see the [0.8 publishing guide](docs/publishing/README.md).

This is an exact-build native mod. A newer Horizon Nightly can change the
functions it targets and may require a new release of ViaHorizon.

## License

[GPL-3.0-or-later](LICENSE). This is an unofficial port of
[ViaFabricPlus](https://github.com/ViaVersion/ViaFabricPlus); upstream authors
and derived protocol data are credited in [third-party notices](THIRD_PARTY_NOTICES.md).
The package includes both notices. No Minecraft assets or Java runtime are bundled.
