// SPDX-License-Identifier: GPL-3.0-or-later
use super::{proxy::Capabilities, vfp_session::PlayCapabilities};
use horizon_client::net::Session;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, Weak},
};

type Owner = OnceLock<Arc<PlayCapabilities>>;
static SESSIONS: OnceLock<Mutex<HashMap<u64, Weak<Owner>>>> = OnceLock::new();

pub fn register(session: &Session, caps: &Capabilities) {
    let mut sessions = SESSIONS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    sessions.retain(|_, caps| caps.strong_count() != 0);
    sessions.insert(session.serial(), Arc::downgrade(caps));
}

fn capabilities(session: &Session) -> Option<Arc<PlayCapabilities>> {
    SESSIONS
        .get()?
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&session.serial())?
        .upgrade()?
        .get()
        .cloned()
}

pub fn full_items(session: &Session) -> bool {
    capabilities(session).is_some_and(|caps| caps.full_items())
}

pub fn legacy_protocol(session: &Session) -> Option<i32> {
    capabilities(session)?.legacy_protocol()
}

pub fn unsequenced_blocks(session: &Session) -> bool {
    capabilities(session).is_some_and(|caps| caps.unsequenced_blocks())
}

pub fn protocol(session: &Session) -> Option<i32> {
    capabilities(session)?.protocol()
}

pub fn legacy_sword(item: &str) -> bool {
    matches!(
        item,
        "minecraft:wooden_sword"
            | "minecraft:stone_sword"
            | "minecraft:iron_sword"
            | "minecraft:golden_sword"
            | "minecraft:diamond_sword"
    )
}

thread_local! {
    pub static SWORD_BLOCK_RENDER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
