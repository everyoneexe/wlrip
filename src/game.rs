use std::{
    sync::Arc,
    thread::sleep,
    time::{Duration, Instant},
};

use utils::{Channel, Mutex};

use crate::{
    config::Config,
    cs2::CS2,
    data::Data,
    message::{GameMessage, GameStatus, UiMessage},
    os::mouse::Mouse,
};

pub struct GameManager {
    channel: Channel<UiMessage, GameMessage>,
    data: Arc<Mutex<Data>>,
    config: Config,
    mouse: Option<Mouse>,
    cs2: CS2,
    #[cfg(feature = "lagdbg")]
    timing: crate::lagdbg::Timing,
}

impl GameManager {
    pub fn new(channel: Channel<UiMessage, GameMessage>, data: Arc<Mutex<Data>>) -> Self {
        let mouse = match Mouse::open() {
            Ok(mouse) => Some(mouse),
            Err(err) => {
                utils::error!("error creating uinput device: {err}");
                utils::error!("uinput kernel module is not loaded, or user is not in input group.");
                utils::error!("continuing in ESP-only mode: aimbot, triggerbot and rcs are disabled.");
                None
            }
        };

        Self {
            channel,
            data,
            config: Config::default(),
            mouse,
            cs2: CS2::new(),
            #[cfg(feature = "lagdbg")]
            timing: crate::lagdbg::Timing::new(),
        }
    }

    fn send_message(&self, message: UiMessage) {
        if self.channel.send(message).is_err() {
            std::process::exit(1);
        }
    }

    pub fn run(&mut self) {
        self.send_message(UiMessage::Status(GameStatus::NotStarted));
        let mut previous_status = GameStatus::NotStarted;
        loop {
            let start = Instant::now();
            while let Ok(message) = self.channel.try_receive() {
                self.config = *message.0;
            }

            let mut is_valid = self.cs2.is_valid();
            if !is_valid {
                if previous_status == GameStatus::Working {
                    self.send_message(UiMessage::Status(GameStatus::NotStarted));
                    previous_status = GameStatus::NotStarted;
                }
                self.cs2.setup();
                is_valid = self.cs2.is_valid();
            }

            if is_valid {
                if previous_status == GameStatus::NotStarted {
                    self.send_message(UiMessage::Status(GameStatus::Working));
                    previous_status = GameStatus::Working;
                }
                self.cs2.run(&self.config, self.mouse.as_mut());

                #[cfg(not(feature = "lagdbg"))]
                {
                    let mut data = self.data.lock();
                    self.cs2.data(&self.config, &mut data);
                }

                #[cfg(feature = "lagdbg")]
                {
                    // measure how long the data() call holds the mutex
                    let data_start = Instant::now();
                    let mut data = self.data.lock();
                    let lock_acquired = data_start.elapsed();
                    self.cs2.data(&self.config, &mut data);
                    drop(data);
                    let data_elapsed = data_start.elapsed();
                    if let Some(line) = self.timing.sample(lock_acquired, data_elapsed) {
                        utils::info!("{line} players={}", self.cs2.player_count());
                    }
                }
            } else {
                *self.data.lock() = Data::default();
            }

            if is_valid {
                let elapsed = start.elapsed();
                if elapsed < self.loop_duration() {
                    sleep(self.loop_duration() - elapsed);
                } else {
                    utils::debug!(
                        "game loop took {} ms (max {} ms)",
                        elapsed.as_millis(),
                        self.loop_duration().as_millis()
                    );
                }
                self.send_message(UiMessage::FrameTime(elapsed));
            } else {
                sleep(Duration::from_secs(5));
            }
        }
    }

    fn loop_duration(&self) -> Duration {
        // The render/overlay thread only draws the latest snapshot this loop
        // writes (it has no process access), so ESP freshness is bounded by how
        // often we read the view matrix here. data() costs <1ms, so a tight loop
        // keeps the view matrix current and stops ESP from lagging behind fast
        // camera turns.
        Duration::from_millis(2)
    }
}
