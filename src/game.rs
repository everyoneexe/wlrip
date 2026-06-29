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
    mouse: Mouse,
    cs2: CS2,
    // TEMP: lag instrumentation
    timing_samples: u32,
    timing_data_accum: Duration,
    timing_lock_accum: Duration,
    timing_last: Instant,
}

impl GameManager {
    pub fn new(channel: Channel<UiMessage, GameMessage>, data: Arc<Mutex<Data>>) -> Self {
        let mouse = match Mouse::open() {
            Ok(mouse) => mouse,
            Err(err) => {
                utils::error!("error creating uinput device: {err}");
                utils::error!("uinput kernel module is not loaded, or user is not in input group.");
                std::process::exit(1);
            }
        };

        Self {
            channel,
            data,
            config: Config::default(),
            mouse,
            cs2: CS2::new(),
            timing_samples: 0,
            timing_data_accum: Duration::ZERO,
            timing_lock_accum: Duration::ZERO,
            timing_last: Instant::now(),
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
                self.cs2.run(&self.config, &mut self.mouse);
                // TEMP: measure how long the data() call holds the mutex
                let data_start = Instant::now();
                let mut data = self.data.lock();
                let lock_acquired = data_start.elapsed();
                self.cs2.data(&self.config, &mut data);
                drop(data);
                let data_elapsed = data_start.elapsed();
                // TEMP: periodic timing dump (~1/sec)
                self.timing_samples += 1;
                self.timing_data_accum += data_elapsed;
                self.timing_lock_accum += lock_acquired;
                if self.timing_last.elapsed() >= Duration::from_secs(1) {
                    let n = self.timing_samples.max(1);
                    utils::info!(
                        "[lagdbg] samples/s={} avg_data()={:.2}ms avg_lock_wait={:.3}ms players={}",
                        self.timing_samples,
                        self.timing_data_accum.as_secs_f32() * 1000.0 / n as f32,
                        self.timing_lock_accum.as_secs_f32() * 1000.0 / n as f32,
                        self.cs2.player_count(),
                    );
                    self.timing_samples = 0;
                    self.timing_data_accum = Duration::ZERO;
                    self.timing_lock_accum = Duration::ZERO;
                    self.timing_last = Instant::now();
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
