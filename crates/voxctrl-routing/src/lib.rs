pub mod loader;
pub mod models;
pub mod router;
pub mod targets;
pub mod timestamp;

pub use loader::{config_dir, load_bindings, load_targets, save_bindings, save_targets};
pub use models::{
    DeliveryResult, DeliveryType, GestureType, HotkeyBinding, OutputTarget,
    TargetProcessingConfig, TestResult, TTS_STOP_BINDING_ID,
};
pub use router::OutputTargetRouter;
pub use targets::{
    build_target, chat_history, parse_voice_command, reset_chat_history, ChatMessage,
    CommandTarget, DeliveryTarget, VoiceCommandParseResult,
};
pub use timestamp::{
    default_file_timestamp_format, render_timestamp, validate_timestamp_format,
    DEFAULT_TIMESTAMP_FORMAT,
};

#[cfg(test)]
mod tests;
