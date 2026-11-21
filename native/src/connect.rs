// SPDX-License-Identifier: GPL-3.0-or-later
fn inject(
    game: &mut crate::game::Game<'_>,
    address: &str,
    name: &str,
    local: &Option<horizon_protocol::connection::Pipe>,
    cookies: &Option<horizon_client::net::Cookies>,
    lan: bool,
    realm: bool,
) -> ::core::ops::ControlFlow<Result<()>> {
    use crate::viahorizon_runtime::{proxy, vfp_capture_session, vfp_core as core};
    use ::core::ops::ControlFlow;

    // Do not intercept single-player, LAN discovery, Realms, or friend tunnels.
    if local.is_some() || lan || realm || address.starts_with(horizon_client::net::FRIEND) {
        return ControlFlow::Continue(());
    }
    let Ok((host, port)) = core::address(address) else {
        return ControlFlow::Continue(()); // Let Horizon report an invalid address.
    };
    if host.contains('\0') {
        return ControlFlow::Continue(());
    }
    let mut config = match core::Config::load_migrating(
        &game.native_data_dir(core::ID).join("config.json"),
        &game.native_data_dir(core::LEGACY_ID).join("config.json"),
    ) {
        Ok((config, _)) => config,
        Err(error) => return ControlFlow::Break(Err(format!("ViaHorizon: {error}").into())),
    };
    if let Some(index) = game
        .setting(core::ID, "server_target")
        .and_then(|setting| setting.trim().parse::<usize>().ok())
        && let Some(target) = crate::viahorizon_runtime::target(index)
    {
        config.target = target;
    }
    if let Some(preview) = game
        .setting(core::ID, "configuration_preview")
        .and_then(|setting| setting.trim().parse::<bool>().ok())
    {
        config.login_preview = preview;
    }
    if !config.enabled || config.selection(&host, port) == core::Selection::Native {
        return ControlFlow::Continue(());
    }
    let auth = game.auth();
    let user = auth.as_ref().map_or_else(
        || {
            (
                name.to_owned(),
                horizon_protocol::crypto::offline_uuid(name),
            )
        },
        |account| (account.name.clone(), account.uuid),
    );
    let (pipe, caps) = match proxy::start(address.to_owned(), auth, cookies.clone(), config) {
        Ok(value) => value,
        Err(error) => return ControlFlow::Break(Err(error.into())),
    };
    game.world_biomes = None;
    game.world_biomes_base = None;
    // Re-enter with a private memory pipe; the guard above uses Horizon's
    // original connection path for the inner call.
    let result = game.connect_remote_session(address, name, Some(pipe), None, lan, realm);
    if result.is_ok() {
        if let Some(remote) = game.remote.as_mut() {
            remote.session.address = address.to_owned();
            if let Some(cookies) = cookies {
                remote
                    .session
                    .cookies
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .extend(cookies.lock().unwrap_or_else(|e| e.into_inner()).clone());
            }
            vfp_capture_session::register(&remote.session, &caps);
        }
        game.server_packs_connect(Some(address), user);
        game.extras_connecting(address, name, lan);
    }
    ControlFlow::Break(result)
}
