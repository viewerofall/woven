#![allow(dead_code)]

//! Unix socket IPC server — listens for commands from woven-ctrl
//! All messages are newline-delimited JSON using types from woven-common::ipc

use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{error, info};
use woven_common::ipc::{IpcCommand, IpcResponse};

pub struct IpcServer {
    pub socket_path: String,
}

impl IpcServer {
    pub fn new() -> Self {
        Self { socket_path: woven_common::ipc::socket_path() }
    }

    /// Spawn the server, call `handler` for each incoming command
    pub async fn serve<F, Fut>(self, handler: Arc<F>) -> Result<()>
    where
    F:   Fn(IpcCommand) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = IpcResponse> + Send,
    {
        // remove stale socket if present
        let _ = std::fs::remove_file(&self.socket_path);
        let listener = UnixListener::bind(&self.socket_path)?;
        info!("IPC server listening on {}", self.socket_path);

        loop {
            let (stream, _) = listener.accept().await?;
            let handler = handler.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, handler).await {
                    error!("IPC connection error: {}", e);
                }
            });
        }
    }
}

async fn handle_connection<F, Fut>(
    stream:  UnixStream,
    handler: Arc<F>,
) -> Result<()>
where
F:   Fn(IpcCommand) -> Fut + Send + Sync + 'static,
Fut: std::future::Future<Output = IpcResponse> + Send,
{
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let cmd: IpcCommand = serde_json::from_str(&line)?;
        let resp            = handler(cmd).await;
        let mut encoded     = serde_json::to_string(&resp)?;
        encoded.push('\n');
        writer.write_all(encoded.as_bytes()).await?;
    }
    Ok(())
}
