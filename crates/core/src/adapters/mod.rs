use crate::model::{CoreEvent, Decision};
use peekaboo_protocol::HookMessage;

pub mod claude;

/// Translates one agent's hook vocabulary to and from Peekaboo's.
pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    /// Stable key for the session this message belongs to, prefixed with the agent id.
    fn session_key(&self, msg: &HookMessage) -> String;
    fn normalize(&self, msg: &HookMessage) -> Vec<CoreEvent>;
    /// Stdout and exit code the hook binary should produce for `decision`.
    fn render_reply(&self, msg: &HookMessage, decision: &Decision) -> (String, i32);
}

pub fn adapter_for(agent: &str) -> Option<&'static dyn AgentAdapter> {
    match agent {
        "claude" => Some(&claude::ClaudeAdapter),
        _ => None,
    }
}

pub(crate) fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(serde_json::Value::as_str).unwrap_or_default().to_string()
}
