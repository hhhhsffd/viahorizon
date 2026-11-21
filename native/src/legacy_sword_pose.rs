// SPDX-License-Identifier: GPL-3.0-or-later
fn inject(
    left: bool,
    inverse_height: f32,
    attack: f32,
    pose: crate::viahorizon_runtime::HandPose,
    original: impl FnOnce(bool, f32, f32, crate::viahorizon_runtime::HandPose) -> glam::Mat4,
) -> glam::Mat4 {
    use crate::viahorizon_runtime::{HandPose, vfp_capture_session::SWORD_BLOCK_RENDER};
    use glam::{Mat4, Vec3};

    if !matches!(pose, HandPose::Using) || !SWORD_BLOCK_RENDER.replace(false) {
        return original(left, inverse_height, attack, pose);
    }
    let sign = if left { -1.0 } else { 1.0 };
    Mat4::from_translation(Vec3::new(sign * 0.56, -0.52 - inverse_height * 0.6, -0.72))
        * Mat4::from_rotation_y((sign * 45.0_f32).to_radians())
        * Mat4::from_translation(Vec3::new(sign * -0.5, 0.2, 0.0))
        * Mat4::from_rotation_y((sign * 30.0_f32).to_radians())
        * Mat4::from_rotation_x(-80.0_f32.to_radians())
        * Mat4::from_rotation_y((sign * 60.0_f32).to_radians())
}
