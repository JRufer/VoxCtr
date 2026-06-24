use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use voxctrl_config::{OllamaConfig, OllamaMode};

// ── Prompts per mode ──────────────────────────────────────────────────────────

fn mode_prompt(mode: &OllamaMode, text: &str) -> String {
    match mode {
        OllamaMode::Clean => format!(
            "Fix grammar and punctuation only. Return only the corrected text, no commentary.\n\nText: {text}"
        ),
        OllamaMode::Formal => format!(
            "Rewrite in formal professional language. Return only the result.\n\nText: {text}"
        ),
        OllamaMode::Casual => format!(
            "Rewrite in casual conversational language. Return only the result.\n\nText: {text}"
        ),
        OllamaMode::Bullet => format!(
            "Convert to a bullet-point list. Return only the list.\n\nText: {text}"
        ),
        OllamaMode::Concise => format!(
            "Summarize concisely in 1-2 sentences. Return only the summary.\n\nText: {text}"
        ),
        OllamaMode::Custom => text.to_string(), // handled separately
    }
}

// ── OpenAI-compatible API types ────────────────────────────────────────────────

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
}

#[derive(Deserialize)]
struct ChatCompletionMessage {
    content: String,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
}

// ── Client ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct OllamaClient {
    config: OllamaConfig,
    http: reqwest::Client,
    available: std::sync::Arc<std::sync::Mutex<Option<bool>>>,
}

impl OllamaClient {
    pub fn new(config: OllamaConfig) -> Self {
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

    fn auth_header(&self) -> Option<String> {
        self.config
            .api_key
            .as_ref()
            .filter(|k| !k.is_empty())
            .map(|k| format!("Bearer {k}"))
    }

    /// Lazily probe if the server is reachable. Cached after first check.
    pub async fn is_available(&self) -> bool {
        {
            let guard = self.available.lock().unwrap();
            if let Some(v) = *guard {
                return v;
            }
        }
        let url = format!("{}/v1/models", self.config.endpoint);
        let mut req = self.http.get(&url).timeout(Duration::from_secs(2));
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        let ok = req
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        *self.available.lock().unwrap() = Some(ok);
        if ok {
            info!("LLM server reachable at {}", self.config.endpoint);
        } else {
            warn!("LLM server not reachable at {}", self.config.endpoint);
        }
        ok
    }

    /// Post-process text through the configured LLM server. Returns original text on any failure.
    pub async fn process(&self, text: &str) -> String {
        if !self.config.enabled {
            return text.to_string();
        }
        if !self.is_available().await {
            return text.to_string();
        }

        let prompt = if self.config.mode == OllamaMode::Custom {
            if let Some(tmpl) = &self.config.custom_prompt {
                if tmpl.contains("{text}") {
                    tmpl.replace("{text}", text)
                } else {
                    format!("{tmpl}\n\n{text}")
                }
            } else {
                return text.to_string();
            }
        } else {
            mode_prompt(&self.config.mode, text)
        };

        let url = format!("{}/v1/chat/completions", self.config.endpoint);
        let req = ChatCompletionRequest {
            model: &self.config.model,
            messages: vec![ChatMessage { role: "user", content: &prompt }],
            stream: false,
            max_tokens: None,
        };

        let mut builder = self.http.post(&url).json(&req);
        if let Some(auth) = self.auth_header() {
            builder = builder.header("Authorization", auth);
        }

        match builder.send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ChatCompletionResponse>().await {
                    Ok(mut body) => {
                        let result = body
                            .choices
                            .pop()
                            .map(|c| c.message.content.trim().to_string())
                            .unwrap_or_else(|| text.to_string());
                        debug!(
                            input_len = text.len(),
                            output_len = result.len(),
                            "LLM processed"
                        );
                        result
                    }
                    Err(e) => {
                        warn!("LLM response parse error: {e}");
                        text.to_string()
                    }
                }
            }
            Ok(resp) => {
                warn!("LLM HTTP {}", resp.status());
                text.to_string()
            }
            Err(e) => {
                warn!("LLM request error: {e}");
                text.to_string()
            }
        }
    }

    /// Run a chat completion with an arbitrary system prompt, prior
    /// conversation history, and a new user message, bypassing the
    /// `mode`-based rewrite templates. Used by the OpenAI API output
    /// target, which lets each target supply its own instruction and
    /// maintain its own running conversation.
    pub async fn complete(
        &self,
        system_prompt: Option<&str>,
        history: &[(String, String)],
        user_text: &str,
        model_override: Option<&str>,
        max_tokens: Option<u32>,
        timeout_secs: Option<u64>,
    ) -> Result<String, String> {
        let model = model_override.unwrap_or(&self.config.model);
        let mut messages = Vec::new();
        if let Some(sp) = system_prompt {
            if !sp.is_empty() {
                messages.push(ChatMessage { role: "system", content: sp });
            }
        }
        for (role, content) in history {
            messages.push(ChatMessage { role, content });
        }
        messages.push(ChatMessage { role: "user", content: user_text });

        let url = format!("{}/v1/chat/completions", self.config.endpoint);
        let req = ChatCompletionRequest { model, messages, stream: false, max_tokens };

        let mut builder = self.http.post(&url).json(&req);
        if let Some(t) = timeout_secs {
            builder = builder.timeout(Duration::from_secs(t));
        }
        if let Some(auth) = self.auth_header() {
            builder = builder.header("Authorization", auth);
        }

        let resp = builder.send().await.map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            let mut body: ChatCompletionResponse =
                resp.json().await.map_err(|e| e.to_string())?;
            body.choices
                .pop()
                .map(|c| c.message.content.trim().to_string())
                .ok_or_else(|| "Empty response from LLM server".to_string())
        } else {
            Err(format!("HTTP error: {}", resp.status()))
        }
    }

    /// Reset the availability cache (e.g., user changed endpoint in settings).
    pub fn reset_availability(&self) {
        *self.available.lock().unwrap() = None;
    }

    /// Retrieve the list of available models from the OpenAI-API-compatible server.
    pub async fn list_models(&self) -> Result<Vec<String>, String> {
        let url = format!("{}/v1/models", self.config.endpoint);
        let mut builder = self.http.get(&url);
        if let Some(auth) = self.auth_header() {
            builder = builder.header("Authorization", auth);
        }
        let resp = builder.send().await.map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            #[derive(serde::Deserialize)]
            struct ModelItem { id: String }
            #[derive(serde::Deserialize)]
            struct ModelsResponse { data: Vec<ModelItem> }
            let models = resp.json::<ModelsResponse>().await.map_err(|e| e.to_string())?;
            Ok(models.data.into_iter().map(|m| m.id).collect())
        } else {
            Err(format!("HTTP error: {}", resp.status()))
        }
    }
}
