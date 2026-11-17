use chrono::Local;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamStatus {
    Disconnected,
    Connecting,
    Connected,
}

impl StreamStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting...",
            Self::Connected => "Connected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamDirection {
    Outgoing,
    Incoming,
    System,
}

#[derive(Debug, Clone)]
pub struct StreamEntry {
    pub timestamp: String,
    pub direction: StreamDirection,
    pub event_name: Option<String>,
    pub payload: String,
}

impl StreamEntry {
    pub fn now(
        direction: StreamDirection,
        event_name: Option<String>,
        payload: impl Into<String>,
    ) -> Self {
        Self {
            timestamp: Local::now().format("%H:%M:%S%.3f").to_string(),
            direction,
            event_name,
            payload: payload.into(),
        }
    }
}
