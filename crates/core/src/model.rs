use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub question: String,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub options: Vec<QuestionOption>,
    #[serde(default)]
    pub multi_select: bool,
}

/// Agent-neutral event vocabulary produced by adapters.
#[derive(Debug, Clone, PartialEq)]
pub enum CoreEvent {
    SessionStarted,
    SessionEnded,
    PromptSubmitted { prompt: String },
    ToolStarted { tool: String, input: Value },
    ToolFinished { tool: String },
    PermissionRequested { tool: String, input: Value },
    QuestionAsked { questions: Vec<Question> },
    Notification { text: String },
    Idle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub question: String,
    pub answer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Decision {
    Allow {
        #[serde(default)]
        always: bool,
    },
    Deny {
        #[serde(default)]
        reason: Option<String>,
    },
    Passthrough,
    Answer { answers: Vec<Answer> },
    /// Keep a stopping session going with a prompt the user typed in Peekaboo.
    Continue { prompt: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    Working,
    WaitingApproval,
    WaitingAnswer,
    Idle,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub key: String,
    pub agent: String,
    pub cwd: Option<String>,
    pub project: String,
    pub state: SessionState,
    pub last_action: Option<String>,
    pub updated_at: u64,
    /// Title from the session's transcript (filled in by the app; the core doesn't read transcripts).
    #[serde(default)]
    pub title: Option<String>,
    /// Terminal device the agent runs on, e.g. `ttys004`.
    #[serde(default)]
    pub tty: Option<String>,
    /// Prompts waiting to be delivered when the agent next stops.
    #[serde(default)]
    pub queued: usize,
    /// Terminal app the agent runs in ("Terminal", "Warp", …), from `TERM_PROGRAM`.
    #[serde(default)]
    pub terminal: Option<String>,
    /// Listen mode: when the agent finishes a turn, Peekaboo holds it for the user's next prompt.
    #[serde(default)]
    pub listening: bool,
    /// Held right now: a prompt sent from Peekaboo is delivered immediately.
    #[serde(default)]
    pub held: bool,
}

/// Friendly name for a `TERM_PROGRAM` value.
pub fn terminal_name(term_program: &str) -> String {
    match term_program {
        "Apple_Terminal" => "Terminal",
        "iTerm.app" => "iTerm2",
        "WarpTerminal" => "Warp",
        "vscode" => "VS Code",
        "ghostty" => "Ghostty",
        "WezTerm" => "WezTerm",
        "tmux" => "tmux",
        other => other,
    }
    .to_string()
}

/// Terminals Peekaboo can type a prompt into directly.
pub fn can_type_into(terminal: Option<&str>) -> bool {
    matches!(terminal, None | Some("Terminal") | Some("iTerm2"))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PendingKind {
    Permission { tool: String, input: Value },
    Question { questions: Vec<Question> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingView {
    pub id: String,
    pub agent: String,
    pub session_key: String,
    pub project: String,
    #[serde(flatten)]
    pub kind: PendingKind,
    pub created_at: u64,
}

/// "Always allow" for one tool in one project; for shell tools, only the exact command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlwaysRule {
    pub agent: String,
    pub tool: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
}

/// Tools whose "Always" is scoped to the exact command rather than the whole tool.
pub const COMMAND_SCOPED_TOOLS: &[&str] = &["Bash", "PowerShell"];

impl AlwaysRule {
    pub fn for_request(agent: &str, tool: &str, input: &Value, cwd: Option<&str>) -> AlwaysRule {
        let command = COMMAND_SCOPED_TOOLS
            .contains(&tool)
            .then(|| input.get("command").and_then(Value::as_str).unwrap_or_default().to_string());
        AlwaysRule { agent: agent.into(), tool: tool.into(), cwd: cwd.map(Into::into), command }
    }

    pub fn matches(&self, agent: &str, tool: &str, input: &Value, cwd: Option<&str>) -> bool {
        self.agent == agent
            && self.tool == tool
            && self.cwd.as_deref() == cwd
            && cwd.is_some()
            && self.command.as_deref().is_none_or(|c| input.get("command").and_then(Value::as_str) == Some(c))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookStatus {
    pub agent: String,
    pub installed: bool,
    /// Our hook entries exist but point at a binary that is gone; Install repairs them.
    pub stale: bool,
    pub config_path: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppState {
    pub sessions: Vec<Session>,
    pub pending: Vec<PendingView>,
    pub rules: Vec<AlwaysRule>,
    pub hooks: Vec<HookStatus>,
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Last path component of `cwd`, used as a human project name.
pub fn project_name(cwd: Option<&str>) -> String {
    cwd.and_then(|c| std::path::Path::new(c).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decision_json_shape() {
        let d: Decision = serde_json::from_str(r#"{"type":"allow","always":true}"#).unwrap();
        assert_eq!(d, Decision::Allow { always: true });
        let d: Decision = serde_json::from_str(r#"{"type":"passthrough"}"#).unwrap();
        assert_eq!(d, Decision::Passthrough);
    }
    #[test]
    fn pending_view_is_flat() {
        let v = PendingView {
            id: "1".into(), agent: "claude".into(), session_key: "k".into(), project: "p".into(),
            kind: PendingKind::Permission { tool: "Bash".into(), input: serde_json::json!({}) }, created_at: 0,
        };
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["kind"], "permission");
        assert_eq!(j["tool"], "Bash");
        assert_eq!(j["sessionKey"], "k");
    }
}
