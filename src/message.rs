use std::{fmt::Display, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{config::Config, ui::grenades::GrenadeList};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GameStatus {
    Working,
    NotStarted,
}

impl Display for GameStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameStatus::Working => write!(f, "Working"),
            GameStatus::NotStarted => write!(f, "Not Started"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GameMessage(pub Box<Config>);

#[derive(Debug, Clone)]
pub enum UiMessage {
    Status(GameStatus),
    FrameTime(Duration),
}

/// Messages pushed from the GUI thread to the overlay render thread.
#[derive(Debug, Clone)]
pub enum OverlayMessage {
    Config(Box<Config>),
    Grenades(Box<GrenadeList>),
}
