#!/usr/bin/env python3
"""Assemble the native .hmod source project against its exact Horizon SDK."""
import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
LEGACY = ROOT / "mixins" / "viahorizon"
NATIVE = ROOT / "native"
STAGE = ROOT / "target" / "native-hmod"

# Some Game implementations moved or split since the legacy .hnmod pin.
GAME = "horizon_client::game::"
TARGETS = {
    "remote_use_item": "remote::use_item",
    "remote_use_item_on": "remote::interaction_prediction",
    "hand_geometry": "frame_geometry",
    "native_use_duration": "native_hooks::base",
    "native_use_animation": "native_hooks::base",
}
RETIRED_HOOKS = {"frame", "connect", "list", "settings", "click", "command"}


def read(path):
    return path.read_text(encoding="utf-8")


def write(path, text):
    path.write_text(text, encoding="utf-8", newline="\n")


def game_source(source):
    source = source.replace("horizon_client::net::vfp_capture_session",
                            "crate::viahorizon_runtime::vfp_capture_session")
    source = source.replace("horizon_world::nbt", "horizon_core::nbt")
    source = source.replace("horizon_world::consumable::UseAnimation",
                            "horizon_core::use_animation::UseAnimation")
    return re.sub(r"horizon_protocol::(vfp_\w+)", r"crate::viahorizon_runtime::\1", source)


def protocol_source(source):
    source = re.sub(r"(?:crate::item::)?Component::read\(", "crate::item_compat::read(", source)
    source = re.sub(r"value\.write\((kind|component::PROFILE), w\)",
                    r"crate::item_compat::write(&value, \1, w)", source)
    source = source.replace("Component::FireworkExplosion(explosion(value)).write(70, &mut w)",
                            "crate::item_compat::write(&Component::FireworkExplosion(explosion(value)), 70, &mut w)")
    source = source.replace(".write(71, &mut w)", ".vfp_write(71, &mut w)")
    if "vfp_named_nbt" in source:
        source = "use crate::buf::{NamedReader as _, NamedWriter as _};\n" + source
    if ".vfp_write(" in source:
        source = "use crate::item_compat::ComponentWrite as _;\n" + source
    return source.replace("crate::", "crate::viahorizon_runtime::").replace(
        "horizon_world::nbt", "horizon_core::nbt")


def enclosed(name, items):
    return f"\npub mod {name} {{\n{items}\n}}\n"


def upstream_entity_code(checkout):
    path = "crates/horizon-protocol/src/entity_data.rs"
    source = read(checkout / path)
    parts = [
        source[source.index(f"fn {start}("):source.index(f"fn {end}(")]
        for start, end in [("read_properties", "write_properties"), ("read_profile", "write_profile"),
                           ("read_value", "write_value")]
    ]
    codec = "\n".join(parts)
    # The translator hands these codecs canonical 26.3 data. Do not invoke
    # the engine's separate older-server mapping a second time.
    codec = re.sub(r"COMPONENT => Value::Text\(if ids::quirks\(\).components_as_json \{.*?\}\),",
                   "COMPONENT => Value::Text(r.nbt()?),", codec, flags=re.S)
    codec = re.sub(r"Some\(if ids::quirks\(\).components_as_json \{.*?\}\)",
                   "Some(r.nbt()?)", codec, flags=re.S)
    codec = re.sub(r"        WOLF_VARIANT if .*?        // VarInt-encoded:",
                   "        // VarInt-encoded:", codec, flags=re.S)
    codec = codec.replace("ids::block_state(r.varint()?)", "r.varint()?")
    return ("// Adapted from Horizon's entity_data.rs (LGPL-3.0-or-later).\n"
            "pub use horizon_protocol::entity_data::*;\n"
            "use horizon_protocol::{Result, item::Slot};\n"
            "use crate::viahorizon_runtime::buf::{Codec, Reader};\n"
            + protocol_source(codec)
            + enclosed("vfp_entities", protocol_source(read(LEGACY / "entities.rs"))))


def releases():
    source = read(LEGACY / "core.rs")
    return re.findall(r'first: "([\d.]+)",\s+last: "([\d.]+)",\s+protocol: \d+,\s+'
                      r'legacy: false,\s+stage: Stage::(?:Gameplay|ConfigurationPreview),', source)


def settings(stage):
    values = [("config", "Use config.json"), ("auto", "Auto detect"), ("native", "Native 26.3")]
    values += [(first, first if first == last else f"{first}–{last}") for first, last in releases()]
    lang = {
        "viahorizon.settings.target": "Server version",
        "viahorizon.settings.target.tooltip": "Server overrides in config.json take priority.",
        "viahorizon.settings.preview": "Configuration preview",
        "viahorizon.settings.preview.tooltip": "Check login and configuration without entering the world.",
    }
    for key, label in values:
        lang[f"viahorizon.target.{key}"] = label
    assets = stage / "assets" / "viahorizon" / "lang"
    assets.mkdir(parents=True, exist_ok=True)
    write(assets / "en_us.json", json.dumps(lang, indent=2, ensure_ascii=False) + "\n")
    choices = json.dumps([f"viahorizon.target.{key}" for key, _ in values])
    entry = read(NATIVE / "src" / "lib.rs").replace("/* choices */ []", choices)
    write(stage / "src" / "lib.rs", entry)
    keys = json.dumps([key for key, _ in values])
    return f"""
pub fn target(index: usize) -> Option<vfp_core::Selection> {{
    const TARGETS: &[&str] = &{keys};
    vfp_core::Selection::parse(TARGETS.get(index)?).ok()
}}
"""


def runtime(stage, checkout, legacy_manifest, version):
    fragments = [
        "// SPDX-License-Identifier: GPL-3.0-or-later\n",
        "pub use horizon_protocol::*;\n",
        "pub use horizon_client::first_person::ItemPose as HandPose;\n",
        "pub fn protocol() -> i32 { horizon_protocol::PROTOCOL_VERSION }\n",
        read(NATIVE / "src" / "codecs.rs"),
        settings(stage),
        enclosed("entity_data", upstream_entity_code(checkout)),
    ]
    for row in legacy_manifest["module"]:
        if row["file"] != "crates/horizon-protocol/src/lib.rs":
            continue
        source = read(LEGACY / row["source"])
        if row["id"] == "core":
            source = re.sub(r'(?m)^pub const VERSION: &str = "[^"]+";',
                            f'pub const VERSION: &str = "{version}";', source)
        if row["id"] == "play":
            # This Nightly decodes FilterMask natively as a long array.
            start = source.index("                let mut bytes = Vec::with_capacity(n * 8);",
                                 source.index('"PLAYER_CHAT" =>'))
            end = source.index("                w.byte_array(&bytes);", start)
            source = source[:start] + (
                "                w.varint(n as i32);\n"
                "                w.bytes(r.bytes(n * 8)?);"
            ) + source[end + len("                w.byte_array(&bytes);"):]
        fragments.append(enclosed(row["name"], protocol_source(source)))
    certificates = read(LEGACY / "certificates.rs").replace(
        "horizon_protocol::{", "crate::viahorizon_runtime::{"
    ).replace("super::AuthSession", "horizon_client::auth::AuthSession").replace(
        "account.access_token)", "account.access_token())")
    fragments.append(enclosed("vfp_certificates", certificates))
    fragments.append(enclosed("proxy", read(NATIVE / "src" / "proxy.rs")))
    fragments.append(enclosed("vfp_capture_session", read(NATIVE / "src" / "capabilities.rs")))
    return "".join(fragments)


def stage(checkout):
    header = read(NATIVE / "horizon-mod.toml")
    mod = tomllib.loads(header)
    sha = mod["native"]["horizon"]
    # Read only the pinned upstream implementation: neither the SDK nor a
    # locally edited Horizon source tree is part of the package.
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=checkout, text=True).strip()
    dirty = subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"],
                                    cwd=checkout, text=True)
    if head != sha or dirty:
        raise SystemExit(f"A pristine Horizon checkout at {sha} is required")
    source_dir = STAGE / "src"
    source_dir.mkdir(parents=True, exist_ok=True)
    raw = read(LEGACY / "horizon-mixin.toml")
    modules = tomllib.loads(raw)
    write(source_dir / "runtime.rs", runtime(STAGE, checkout, modules, mod["version"]))
    for name in ("connect.rs", "legacy_sword_pose.rs"):
        write(source_dir / name, read(NATIVE / "src" / name))
    write(source_dir / "item_support.rs", game_source(read(LEGACY / "item_support.rs")))
    for match in re.finditer(r"^\[\[injection\]\]\n.*?(?=^\[\[|\Z)", raw, re.S | re.M):
        block = match[0]
        row = tomllib.loads(block)["injection"][0]
        target = row["target"]
        if not target.startswith(GAME) or row["id"] in RETIRED_HOOKS:
            continue
        method = target.rsplit("::", 1)[1]
        if method in TARGETS:
            target = f"{GAME}{TARGETS[method]}::Game<'_>::{method}"
        helper = row["helper"]
        header += "\n" + game_source(block.replace("[[injection]]", "[[native.mixin]]").replace(
            row["target"], target).replace(f'helper = "{helper}"', f'helper = "src/{helper}"'))
        write(source_dir / helper, game_source(read(LEGACY / helper)))
    write(STAGE / "horizon-mod.toml", header)
    shutil.copyfile(NATIVE / "Cargo.toml", STAGE / "Cargo.toml")
    shutil.copyfile(ROOT / "LICENSE", STAGE / "LICENSE")
    shutil.copyfile(ROOT / "THIRD_PARTY_NOTICES.md", STAGE / "NOTICE.md")
    shutil.copyfile(checkout / "LICENSE", STAGE / "LICENSE-Horizon-LGPL-3.0.txt")
    # Fail rather than allowing stale source files to enter the .hmod.
    package = tomllib.loads(header)["native"]
    expected = {STAGE / "src" / "lib.rs"}
    expected.update(STAGE / item["source"] for item in package["module"])
    expected.update(STAGE / item["helper"] for item in package["mixin"])
    unexpected = set(source_dir.rglob("*")) - expected
    if unexpected:
        raise SystemExit("Unexpected staged source: " + ", ".join(map(str, sorted(unexpected))))
    print(STAGE)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--horizon", required=True, type=Path, help="pristine pinned Horizon checkout")
    stage(parser.parse_args().horizon.resolve())
