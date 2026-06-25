use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use voxctrl_config::{OpenAiConfig, OpenAiMode};

// ── Preset system prompts ───────────────────────────────────────────────────

/// System-prompt text for each built-in preset. `Custom` has no preset and
/// returns an empty string (the user prompt carries the full instruction).
pub fn preset_system_prompt(mode: &OpenAiMode) -> &'static str {
    match mode {
        OpenAiMode::Clean => {
            "Fix grammar and punctuation only. Return only the corrected text, no commentary."
        }
        OpenAiMode::Formal => {
            "Rewrite the user's text in formal professional language. Return only the result."
        }
        OpenAiMode::Casual => {
            "Rewrite the user's text in casual conversational language. Return only the result."
        }
        OpenAiMode::Bullet => {
            "Convert the user's text to a bullet-point list. Return only the list."
        }
        OpenAiMode::Concise => {
            "Summarize the user's text concisely in 1-2 sentences. Return only the summary."
        }
        OpenAiMode::Custom => "",
    }
}

// ── OpenAI-compatible API types ─────────────────────────────────────────────

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

/// Normalize a user-supplied endpoint into an OpenAI-style base URL ending in
/// `/v1`. Accepts values with or without a trailing slash or existing `/v1`.
fn api_base(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

// ── Client ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct OpenAiClient {
    config: OpenAiConfig,
    http: reqwest::Client,
    available: std::sync::Arc<std::sync::Mutex<Option<bool>>>,
}

impl OpenAiClient {
    pub fn new(config: OpenAiConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("reqwest client");
        Self {
            config,
            http,
            available: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Attach the configured API key as a Bearer token, if one is set.
    fn with_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.config.api_key.as_deref() {
            Some(key) if !key.trim().is_empty() => req.bearer_auth(key.trim()),
            _ => req,
        }
    }

    /// Lazily probe if the API server is reachable. Cached after first check.
    pub async fn is_available(&self) -> bool {
        {
            let guard = self.available.lock().unwrap();
            if let Some(v) = *guard {
                return v;
            }
        }
        let url = format!("{}/models", api_base(&self.config.endpoint));
        let ok = self
            .with_auth(self.http.get(&url))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        *self.available.lock().unwrap() = Some(ok);
        if ok {
            info!("OpenAI API reachable at {}", self.config.endpoint);
        } else {
            warn!("OpenAI API not reachable at {}", self.config.endpoint);
        }
        ok
    }

    /// Post-process text through the OpenAI API. Returns original text on any failure.
    pub async fn process(&self, text: &str) -> String {
        if !self.config.enabled {
            return text.to_string();
        }
        if !self.is_available().await {
            return text.to_string();
        }

        // Build the user message from the configured template, substituting the
        // dictated text into the "{text}" placeholder.
        let template = if self.config.user_prompt.trim().is_empty() {
            "{text}"
        } else {
            self.config.user_prompt.as_str()
        };
        let user_content = if template.contains("{text}") {
            template.replace("{text}", text)
        } else {
            // Be forgiving if the user dropped the placeholder: append the text.
            format!("{template}\n\n{text}")
        };

        let system_content = self.config.system_prompt.trim();
        let mut messages = Vec::with_capacity(2);
        if !system_content.is_empty() {
            messages.push(ChatMessage {
                role: "system",
                content: system_content,
            });
        }
        messages.push(ChatMessage {
            role: "user",
            content: &user_content,
        });

        let url = format!("{}/chat/completions", api_base(&self.config.endpoint));
        let req = ChatRequest {
            model: &self.config.model,
            messages,
            stream: false,
        };

        match self.with_auth(self.http.post(&url).json(&req)).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ChatResponse>().await {
                    Ok(body) => {
                        let result = body
                            .choices
                            .into_iter()
                            .next()
                            .map(|c| c.message.content.trim().to_string())
                            .unwrap_or_default();
                        if result.is_empty() {
                            warn!("OpenAI API returned no content");
                            return text.to_string();
                        }
                        debug!(
                            input_len = text.len(),
                            output_len = result.len(),
                            "OpenAI API processed"
                        );
                        result
                    }
                    Err(e) => {
                        warn!("OpenAI API response parse error: {e}");
                        text.to_string()
                    }
                }
            }
            Ok(resp) => {
                warn!("OpenAI API HTTP {}", resp.status());
                text.to_string()
            }
            Err(e) => {
                warn!("OpenAI API request error: {e}");
                text.to_string()
            }
        }
    }

    /// Reset the availability cache (e.g., user changed endpoint in settings).
    pub fn reset_availability(&self) {
        *self.available.lock().unwrap() = None;
    }

    /// Retrieve the list of available models from the OpenAI-compatible server.
    pub async fn list_models(&self) -> Result<Vec<String>, String> {
        let url = format!("{}/models", api_base(&self.config.endpoint));
        let resp = self
            .with_auth(self.http.get(&url))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            #[derive(serde::Deserialize)]
            struct ModelItem { id: String }
            #[derive(serde::Deserialize)]
            struct ModelsResponse { data: Vec<ModelItem> }
            let body = resp.json::<ModelsResponse>().await.map_err(|e| e.to_string())?;
            Ok(body.data.into_iter().map(|m| m.id).collect())
        } else {
            Err(format!("HTTP error: {}", resp.status()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{api_base, preset_system_prompt};
    use voxctrl_config::OpenAiMode;

    #[test]
    fn api_base_appends_v1() {
        assert_eq!(api_base("http://localhost:11434"), "http://localhost:11434/v1");
        assert_eq!(api_base("http://localhost:11434/"), "http://localhost:11434/v1");
    }

    #[test]
    fn api_base_preserves_existing_v1() {
        assert_eq!(api_base("https://api.openai.com/v1"), "https://api.openai.com/v1");
        assert_eq!(api_base("https://api.openai.com/v1/"), "https://api.openai.com/v1");
    }

    #[test]
    fn preset_custom_has_no_system_prompt() {
        assert_eq!(preset_system_prompt(&OpenAiMode::Custom), "");
        assert!(!preset_system_prompt(&OpenAiMode::Clean).is_empty());
        assert!(preset_system_prompt(&OpenAiMode::Formal).contains("formal"));
    }
}
