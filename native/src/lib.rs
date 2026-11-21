// SPDX-License-Identifier: GPL-3.0-or-later
use mod_api::{Registrar, RegistrationContext, SettingSpec};

#[derive(Default)]
struct Mod;

#[mod_api::horizon_mod]
impl mod_api::Hooks for Mod {
    fn register(&self, registrar: &mut dyn Registrar, _: &RegistrationContext) {
        let _ = registrar.register_setting(
            SettingSpec::choice(
                "server_target",
                "viahorizon.settings.target",
                /* choices */ [],
                0,
            )
            .tooltip("viahorizon.settings.target.tooltip"),
        );
        let _ = registrar.register_setting(
            SettingSpec::toggle(
                "configuration_preview",
                "viahorizon.settings.preview",
                false,
            )
            .tooltip("viahorizon.settings.preview.tooltip"),
        );
    }
}
