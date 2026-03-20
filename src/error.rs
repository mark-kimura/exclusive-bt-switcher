use std::fmt;

#[derive(Debug)]
#[allow(dead_code)]
pub enum AppError {
    Bluetooth(String),
    Audio(String),
    State(String),
    Dbus(zbus::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Bluetooth(msg) => write!(f, "Bluetooth error: {msg}"),
            AppError::Audio(msg) => write!(f, "Audio error: {msg}"),
            AppError::State(msg) => write!(f, "State error: {msg}"),
            AppError::Dbus(e) => write!(f, "D-Bus error: {e}"),
            AppError::Io(e) => write!(f, "IO error: {e}"),
            AppError::Json(e) => write!(f, "JSON error: {e}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<zbus::Error> for AppError {
    fn from(e: zbus::Error) -> Self {
        AppError::Dbus(e)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Json(e)
    }
}
