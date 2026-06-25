use std::fs::File;
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

pub static STARTUP_COMPLETE: AtomicBool = AtomicBool::new(false);

struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }
}

pub struct StartupErrorLayer {
    file: Mutex<File>,
}

impl StartupErrorLayer {
    pub fn new(path: std::path::PathBuf) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl<S: Subscriber> Layer<S> for StartupErrorLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let level = metadata.level();

        let mut visitor = MessageVisitor { message: String::new() };
        event.record(&mut visitor);

        let msg = visitor.message;

        // Apply strict filtering rules to prevent any user text from leaking into the log.
        let lower_msg = msg.to_lowercase();
        if lower_msg.contains("received transcription")
            || lower_msg.contains("delivered target_id")
            || lower_msg.contains("transcribe")
            || lower_msg.contains("transcription")
            || lower_msg.contains("speaking")
            || lower_msg.contains("speak")
            || lower_msg.contains("openai")
            || lower_msg.contains("payload")
            || lower_msg.contains("status-tick")
        {
            return;
        }

        let is_error_or_warn = *level == tracing::Level::ERROR || *level == tracing::Level::WARN;
        let is_startup = !STARTUP_COMPLETE.load(Ordering::SeqCst);

        if is_error_or_warn || (is_startup && *level == tracing::Level::INFO) {
            let timestamp = chrono::Utc::now().to_rfc3339();
            let log_line = format!(
                "{} [{}] {}: {}\n",
                timestamp,
                level,
                metadata.target(),
                msg
            );
            if let Ok(mut file) = self.file.lock() {
                let _ = file.write_all(log_line.as_bytes());
                let _ = file.flush();
            }
        }
    }
}
