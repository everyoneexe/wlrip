use glam::{Vec2, Vec3, vec2};

use crate::{
    config::Config,
    cs2::{
        CS2,
        bones::Bones,
        entity::{player::Player, weapon_class::WeaponClass},
    },
    math::{angles_to_fov, vec2_clamp},
    os::mouse::Mouse,
};

#[derive(Debug, Default)]
pub struct Aimbot {
    pub active: bool,
    inertia: Vec2,
}

impl CS2 {
    pub fn aimbot(&mut self, config: &Config, mouse: &mut Mouse) -> bool {
        let hotkey = config.aim.aimbot_hotkey;
        let config = self.aimbot_config(config);

        if !config.enabled {
            return false;
        }

        if !Self::check_hotkey(&self.input, config.mode, hotkey, &mut self.aim.active) {
            return false;
        }

        let Some(target) = &self.target.player else {
            return false;
        };

        if !target.is_valid(self) {
            return false;
        }

        let Some(local_player) = Player::local_player(self) else {
            return false;
        };

        let weapon_class = local_player.weapon_class(self);
        let disallowed_weapons = [
            WeaponClass::Unknown,
            WeaponClass::Knife,
            WeaponClass::Grenade,
        ];
        if disallowed_weapons.contains(&weapon_class) {
            return false;
        }

        if config.flash_check && local_player.is_flashed(self) {
            return false;
        }

        if config.visibility_check
            && !target.visible(self, &local_player)
            && !(config.autowall && target.penetrable(self, &local_player, 1.0))
        {
            return false;
        }

        if local_player.shots_fired(self) < config.start_bullet {
            return false;
        }

        let target_angle = {
            let mut smallest_fov = 360.0;
            let mut smallest_angle = glam::Vec2::ZERO;
            let target_velocity = target.velocity(self);
            let prediction_time = 0.05;
            let eye_pos = local_player.eye_position(self);
            let view_angles = local_player.view_angles(self);
            for bone in &config.bones {
                let bone_pos =
                    target.bone_position(self, bone.u64()) + target_velocity * prediction_time;

                // For the head, sample a few points around the bone center and
                // aim at the closest *visible* one, so we hit the exposed part of
                // a head peeking around cover instead of the occluded center.
                let multipoint_active =
                    config.multipoint && *bone == Bones::Head && self.bvh.is_some();
                let candidates = if multipoint_active {
                    head_sample_points(eye_pos, bone_pos, config.multipoint_radius)
                } else {
                    vec![bone_pos]
                };

                for point in candidates {
                    // Only filter by line of sight when multipointing; a single
                    // center point keeps the original behavior for non-head
                    // bones and the no-BVH case.
                    if multipoint_active
                        && let Some(bvh) = &self.bvh
                        && !bvh.has_line_of_sight(eye_pos, point)
                    {
                        continue;
                    }

                    let angle =
                        self.angle_to_target(&local_player, &point, &self.target.previous_aim_punch);
                    let fov = angles_to_fov(&view_angles, &angle);
                    if fov < smallest_fov {
                        smallest_fov = fov;
                        smallest_angle = angle;
                    }
                }
            }

            smallest_angle
        };

        let view_angles = local_player.view_angles(self);
        if angles_to_fov(&view_angles, &target_angle)
            > (config.fov
                * if config.distance_adjusted_fov {
                    self.distance_scale(self.target.distance)
                } else {
                    1.0
                })
        {
            return false;
        }

        let mut aim_angles = view_angles - target_angle;
        if aim_angles.y < -180.0 {
            aim_angles.y += 360.0
        }
        vec2_clamp(&mut aim_angles);

        let sensitivity = self.get_sensitivity() * local_player.fov_multiplier(self);

        let mouse_angles = vec2(
            aim_angles.y / sensitivity * 45.45,
            -aim_angles.x / sensitivity * 45.45,
        ) / (config.smooth + 1.0).clamp(1.0, 20.0);

        let alpha = 1.0 - config.inertia.clamp(0.0, 1.0) * 0.5;
        self.aim.inertia += (mouse_angles - self.aim.inertia) * alpha;
        mouse.move_rel(self.aim.inertia);

        self.recoil.previous = local_player.aim_punch(self);

        true
    }
}

/// Sample points around the head bone center, laid out on the plane facing the
/// shooter (center + up/down/left/right). Aiming at the closest visible one lets
/// the aimbot hit a head that is only partially exposed around cover.
fn head_sample_points(eye: Vec3, head_center: Vec3, radius: f32) -> Vec<Vec3> {
    let forward = (head_center - eye).normalize_or_zero();
    if forward == Vec3::ZERO {
        return vec![head_center];
    }

    // Build an up/right basis perpendicular to the view ray so the samples sit
    // on the face of the head pointing toward us.
    let world_up = Vec3::Z;
    let right = forward.cross(world_up).normalize_or_zero();
    let up = if right == Vec3::ZERO {
        // Looking nearly straight up/down: fall back to an arbitrary basis.
        Vec3::X
    } else {
        right.cross(forward).normalize_or_zero()
    };
    let right = if right == Vec3::ZERO { Vec3::Y } else { right };

    vec![
        head_center,
        head_center + up * radius,
        head_center - up * radius,
        head_center + right * radius,
        head_center - right * radius,
    ]
}
