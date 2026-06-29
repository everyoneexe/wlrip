use strum::IntoEnumIterator as _;

use crate::{
    config::Config,
    cs2::{CS2, bones::Bones, entity::player::Player},
    data::{ShotEntry, ShotLogData},
    math::angles_to_fov,
};

/// Maximum number of recent shots kept for the HUD list.
const MAX_ENTRIES: usize = 8;

#[derive(Debug, Default)]
pub struct ShotLog {
    /// Last observed `m_iShotsFired` so we can detect new shots.
    last_shots_fired: i32,
    entries: Vec<ShotEntry>,
    total: u32,
    headshots: u32,
}

impl ShotLog {
    fn record(&mut self, entry: ShotEntry) {
        self.total += 1;
        if entry.headshot {
            self.headshots += 1;
        }
        self.entries.push(entry);
        if self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
        }
    }

    fn reset(&mut self) {
        self.last_shots_fired = 0;
    }

    pub fn snapshot(&self) -> ShotLogData {
        ShotLogData {
            entries: self.entries.clone(),
            total: self.total,
            headshots: self.headshots,
        }
    }
}

impl CS2 {
    /// Detect newly fired bullets and classify which bone of the crosshair
    /// target they were aimed closest to. This is a best-effort classification
    /// based on aim direction, not a confirmed damage event.
    pub fn shot_log(&mut self, config: &Config) {
        if !config.hud.shot_log {
            return;
        }

        let Some(local_player) = Player::local_player(self) else {
            self.shot_log.reset();
            return;
        };

        let shots_fired = local_player.shots_fired(self);

        // Reset tracking when the counter drops (new magazine / round / respawn).
        if shots_fired < self.shot_log.last_shots_fired {
            self.shot_log.last_shots_fired = shots_fired;
            return;
        }

        let new_shots = shots_fired - self.shot_log.last_shots_fired;
        self.shot_log.last_shots_fired = shots_fired;

        if new_shots <= 0 {
            return;
        }

        // Only classify when actually aiming at an enemy under the crosshair.
        let Some(target) = local_player.crosshair_entity(self) else {
            return;
        };
        if !self.is_ffa() && target.team(self) == local_player.team(self) {
            return;
        }

        let eye_position = local_player.eye_position(self);
        let view_angles = local_player.view_angles(self);
        let aim_punch = local_player.aim_punch(self);

        // Find the bone whose direction is closest to where we are looking.
        let mut best_fov = f32::MAX;
        let mut best_bone = Bones::Spine2;
        let mut best_distance = 0.0;
        for bone in Bones::iter() {
            let bone_pos = target.bone_position(self, bone.u64());
            let angle = self.angle_to_target(&local_player, &bone_pos, &aim_punch);
            let fov = angles_to_fov(&view_angles, &angle);
            if fov < best_fov {
                best_fov = fov;
                best_bone = bone;
                best_distance = eye_position.distance(bone_pos);
            }
        }

        let headshot = matches!(best_bone, Bones::Head | Bones::Neck);

        // Each tick may cover more than one bullet (high fire rate); attribute
        // them all to the same classified bone.
        for _ in 0..new_shots.min(MAX_ENTRIES as i32) {
            self.shot_log.record(ShotEntry {
                headshot,
                bone: best_bone,
                distance: best_distance,
            });
        }
    }

    pub fn shot_log_data(&self) -> ShotLogData {
        self.shot_log.snapshot()
    }
}
