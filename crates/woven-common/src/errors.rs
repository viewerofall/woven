use thiserror::Error;

#[derive(Debug, Error)]
pub enum WovenError {
    #[error("Compositor error: {0}")]
    Compositor(String),

    #[error("Lua error: {0}")]
    Lua(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("IPC error: {0}")]
    Ipc(String),

    #[error("Render error: {0}")]
    Render(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
