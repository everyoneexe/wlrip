use std::time::{Duration, Instant};

use glam::Vec2;
use rand::rng;

use crate::{
    config::Config,
    cs2::{
        CS2,
        bones::Bones,
        entity::{player::Player, weapon_class::WeaponClass},
    },
    math::angles_to_fov,
    os::mouse::Mouse,
};

#[derive(Debug, Default)]
pub struct Triggerbot {
    shot_start: Option<Instant>,
    shot_end: Option<Instant>,
    cooldown_until: Option<Instant>,
    pending_cooldown: Option<Duration>,
    pub active: bool,
}

impl CS2 {
    pub fn triggerbot(&mut self, config: &Config) {
        let hotkey = config.aim.triggerbot_hotkey;
        let config = self.triggerbot_config(config);

        if !config.enabled {
            return;
        }

        if !Self::check_hotkey(&self.input, config.mode, hotkey, &mut self.trigger.active) {
            return;
        }

        if self.trigger.shot_start.is_some() || self.trigger.shot_end.is_some() {
            return;
        }

        // Refire cooldown: wait a randomized interval after the previous shot
        // instead of firing again the instant the hold duration elapses.
        if let Some(cooldown_until) = self.trigger.cooldown_until {
            if Instant::now() < cooldown_until {
                return;
            }
            self.trigger.cooldown_until = None;
        }

        let Some(local_player) = Player::local_player(self) else {
            return;
        };

        if config.flash_check && local_player.is_flashed(self) {
            return;
        }

        if config.scope_check
            && local_player.weapon_class(self) == WeaponClass::Sniper
            && !local_player.is_scoped(self)
        {
            return;
        }

        if config.velocity_check && local_player.velocity(self).length() > config.velocity_threshold
        {
            return;
        }

        let Some(player) = local_player.crosshair_entity(self) else {
            return;
        };

        if !self.is_ffa() && player.team(self) == local_player.team(self) {
            return;
        }

        if config.head_only {
            let head = player.bone_position(self, Bones::Head.u64());

            let target_angle = self.angle_to_target(&local_player, &head, &Vec2::ZERO);
            let view_angles = local_player.view_angles(self);
            let fov = angles_to_fov(&view_angles, &target_angle);

            let head_radius_fov =
                3.5 / (local_player.position(self) - player.position(self)).length() * 100.0;

            if fov > head_radius_fov {
                return;
            }
        }

        let mean = (*config.delay.start() + *config.delay.end()) as f32 / 2.0;
        let std_dev = (*config.delay.end() - *config.delay.start()) as f32 / 2.0;

        let normal = rand_distr::Normal::new(mean, std_dev).unwrap();
        use rand_distr::Distribution as _;
        let delay = normal.sample(&mut rng()).max(0.0) as u64;

        let now = Instant::now();
        let delay = Duration::from_millis(delay);
        self.trigger.shot_start = Some(now + delay);
        self.trigger.shot_end = Some(now + delay + Duration::from_millis(config.shot_duration));

        // Sample the post-shot cooldown now (config is in scope); it is applied
        // once the shot is released in triggerbot_shoot.
        let cd_mean =
            (*config.after_shot_delay.start() + *config.after_shot_delay.end()) as f32 / 2.0;
        let cd_std_dev =
            (*config.after_shot_delay.end() - *config.after_shot_delay.start()) as f32 / 2.0;
        let cooldown = if cd_std_dev > 0.0 {
            rand_distr::Normal::new(cd_mean, cd_std_dev)
                .unwrap()
                .sample(&mut rng())
                .max(0.0) as u64
        } else {
            cd_mean as u64
        };
        self.trigger.pending_cooldown = Some(Duration::from_millis(cooldown));
    }

    pub fn triggerbot_shoot(&mut self, mouse: &mut Mouse) {
        let now = Instant::now();

        if let Some(shot_time) = self.trigger.shot_start
            && now >= shot_time
        {
            mouse.left_press();
            self.trigger.shot_start = None;
        }

        if let Some(shot_end) = self.trigger.shot_end
            && now >= shot_end
        {
            mouse.left_release();
            self.trigger.shot_end = None;

            // Start the refire cooldown from the moment the shot is released.
            if let Some(cooldown) = self.trigger.pending_cooldown.take() {
                self.trigger.cooldown_until = Some(now + cooldown);
            }
        }
    }
}
