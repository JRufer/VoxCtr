//! Watches a named pipe (FIFO) for lines of text and speaks each one as it
//! arrives. Used to let external tools/scripts trigger TTS without going
//! through the MCP server.

use std::time::Duration;

use tracing::{info, warn};

use crate::engine::TtsEngineHandle;

pub async fn run_fifo_responder(fifo_path: String, tts: TtsEngineHandle) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    info!("FIFO responder watching {fifo_path}");
    loop {
        while !std::path::Path::new(&fifo_path).exists() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        match tokio::fs::File::open(&fifo_path).await {
            Ok(file) => {
                let mut lines = BufReader::new(file).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim().to_string();
                    if !line.is_empty() {
                        tts.speak(line);
                    }
                }
            }
            Err(e) => {
                warn!("FIFO open error {fifo_path}: {e}; retrying");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}
