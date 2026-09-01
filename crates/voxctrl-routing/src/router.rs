use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;
use tracing::{error, info};

use crate::{
    models::{DeliveryResult, DeliveryType, OutputTarget},
    targets::{build_target, notify_command_trigger, parse_voice_command, DeliveryTarget, InjectTarget},
};

pub struct OutputTargetRouter {
    targets: Arc<RwLock<HashMap<String, Box<dyn DeliveryTarget>>>>,
    configs: Arc<RwLock<Vec<OutputTarget>>>,
}

impl OutputTargetRouter {
    pub fn new(targets: Vec<OutputTarget>) -> Self {
        let configs = Arc::new(RwLock::new(targets.clone()));
        let map = targets
            .into_iter()
            .map(|t| (t.id.clone(), build_target(t)))
            .collect();
        Self {
            targets: Arc::new(RwLock::new(map)),
            configs,
        }
    }

    /// Deliver text to the named target. Returns the result, or an error
    /// result if the target id is unknown.
    pub async fn deliver(&self, target_id: &str, text: &str) -> DeliveryResult {
        let configs_guard = self.configs.read().await;
        let target_config = configs_guard.iter().find(|t| t.id == target_id).cloned();

        if let Some(ref cfg) = target_config {
            if cfg.delivery == DeliveryType::Command {
                info!(
                    from_target = target_id,
                    incoming_text = text,
                    command_template = ?cfg.command,
                    "Activating Voice Command Router target"
                );
                if let Some(parsed) = parse_voice_command(text, &configs_guard) {
                    info!(
                        from_target = target_id,
                        matched_target = %parsed.matched_target_id,
                        payload = %parsed.payload,
                        "Voice command parsed and rerouted"
                    );
                    let matched_id = parsed.matched_target_id.clone();
                    let payload = parsed.payload;
                    let matched_label = configs_guard
                        .iter()
                        .find(|t| t.id == matched_id)
                        .map(|t| if t.label.is_empty() { t.id.clone() } else { t.label.clone() })
                        .unwrap_or_else(|| matched_id.clone());
                    notify_command_trigger(&matched_label, &payload);
                    drop(configs_guard);
                    return Box::pin(self.deliver(&matched_id, &payload)).await;
                } else {
                    info!(
                        from_target = target_id,
                        incoming_text = text,
                        "Voice command trigger not matched; falling back to direct text injection"
                    );
                    drop(configs_guard);
                    let inject_target = InjectTarget(cfg.clone());
                    return inject_target.deliver(text).await;
                }
            } else if let Some(parsed) = parse_voice_command(text, &configs_guard) {
                info!(
                    from_target = target_id,
                    matched_target = %parsed.matched_target_id,
                    payload = %parsed.payload,
                    "Voice command trigger detected and rerouted"
                );
                let matched_id = parsed.matched_target_id.clone();
                let payload = parsed.payload;
                let matched_label = configs_guard
                    .iter()
                    .find(|t| t.id == matched_id)
                    .map(|t| if t.label.is_empty() { t.id.clone() } else { t.label.clone() })
                    .unwrap_or_else(|| matched_id.clone());
                notify_command_trigger(&matched_label, &payload);
                drop(configs_guard);
                return Box::pin(self.deliver(&matched_id, &payload)).await;
            }
        }
        drop(configs_guard);

        let guard = self.targets.read().await;
        match guard.get(target_id) {
            Some(target) => {
                let result = target.deliver(text).await;
                if !result.success {
                    error!(
                        target_id,
                        error = ?result.error,
                        "Delivery failed"
                    );
                } else {
                    info!(target_id, bytes = text.len(), "Delivered");
                }
                result
            }
            None => {
                error!(target_id, "Unknown target");
                DeliveryResult::err(format!("Unknown target: {target_id}"))
            }
        }
    }

    /// Hot-reload: replace all targets atomically.
    pub async fn reload(&self, targets: Vec<OutputTarget>) {
        let map: HashMap<_, _> = targets
            .iter()
            .cloned()
            .map(|t| (t.id.clone(), build_target(t)))
            .collect();
        let mut configs_guard = self.configs.write().await;
        *configs_guard = targets;
        let mut guard = self.targets.write().await;
        *guard = map;
        info!("Router reloaded ({} targets)", guard.len());
    }

    pub async fn target_ids(&self) -> Vec<String> {
        self.targets.read().await.keys().cloned().collect()
    }
}
