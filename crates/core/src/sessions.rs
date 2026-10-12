use crate::model::{project_name, CoreEvent, Session, SessionState};
use serde_json::Value;
use std::collections::HashMap;

pub const STALE_AFTER_SECS: u64 = 30 * 60;

#[derive(Default)]
pub struct SessionStore {
    sessions: HashMap<String, Session>,
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn tool_summary(tool: &str, input: &Value) -> String {
    let detail = ["command", "file_path", "path", "pattern", "url"]
        .iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str));
    match detail {
        Some(d) => truncate(&format!("{tool}: {d}"), 60),
        None => tool.to_string(),
    }
}

impl SessionStore {
    pub fn apply(&mut self, key: &str, agent: &str, cwd: Option<&str>, ev: &CoreEvent, now: u64) {
        let s = self.sessions.entry(key.to_string()).or_insert_with(|| Session {
            key: key.to_string(),
            agent: agent.to_string(),
            cwd: cwd.map(str::to_string),
            project: project_name(cwd),
            state: SessionState::Working,
            last_action: None,
            updated_at: now,
            title: None,
            tty: None,
            queued: 0,
        });
        if s.cwd.is_none() && cwd.is_some() {
            s.cwd = cwd.map(str::to_string);
            s.project = project_name(cwd);
        }
        s.updated_at = now;
        let (state, action) = match ev {
            CoreEvent::SessionStarted => (SessionState::Idle, Some("Session started".to_string())),
            CoreEvent::SessionEnded => (SessionState::Ended, None),
            CoreEvent::PromptSubmitted { prompt } => (SessionState::Working, Some(truncate(prompt, 60))),
            CoreEvent::ToolStarted { tool, input } => (SessionState::Working, Some(tool_summary(tool, input))),
            CoreEvent::ToolFinished { .. } => (SessionState::Working, None),
            CoreEvent::PermissionRequested { tool, .. } => {
                (SessionState::WaitingApproval, Some(format!("Needs approval: {tool}")))
            }
            CoreEvent::QuestionAsked { .. } => (SessionState::WaitingAnswer, Some("Has a question".to_string())),
            CoreEvent::Notification { text } => (s.state, Some(truncate(text, 60))),
            CoreEvent::Idle => (SessionState::Idle, Some("Done".to_string())),
        };
        s.state = state;
        if action.is_some() {
            s.last_action = action;
        }
    }

    pub fn set_state(&mut self, key: &str, state: SessionState, now: u64) {
        if let Some(s) = self.sessions.get_mut(key) {
            s.state = state;
            s.updated_at = now;
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.sessions.get(key).is_some_and(|s| s.state != SessionState::Ended)
    }

    pub fn set_tty(&mut self, key: &str, tty: String) {
        if let Some(s) = self.sessions.get_mut(key) {
            s.tty = Some(tty);
        }
    }

    pub fn set_last_action(&mut self, key: &str, action: &str) {
        if let Some(s) = self.sessions.get_mut(key) {
            s.last_action = Some(action.to_string());
        }
    }

    pub fn tty(&self, key: &str) -> Option<String> {
        self.sessions.get(key).and_then(|s| s.tty.clone())
    }

    pub fn list(&self, now: u64) -> Vec<Session> {
        let mut v: Vec<Session> = self
            .sessions
            .values()
            .filter(|s| s.state != SessionState::Ended && now.saturating_sub(s.updated_at) <= STALE_AFTER_SECS)
            .cloned()
            .collect();
        v.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lifecycle_transitions() {
        let mut s = SessionStore::default();
        s.apply("claude:a", "claude", Some("/w/shop"), &CoreEvent::SessionStarted, 1);
        assert_eq!(s.list(1)[0].state, SessionState::Idle);
        assert_eq!(s.list(1)[0].project, "shop");
        s.apply("claude:a", "claude", None, &CoreEvent::ToolStarted { tool: "Bash".into(), input: json!({"command":"npm test"}) }, 2);
        let l = s.list(2);
        assert_eq!(l[0].state, SessionState::Working);
        assert_eq!(l[0].last_action.as_deref(), Some("Bash: npm test"));
        s.apply("claude:a", "claude", None, &CoreEvent::PermissionRequested { tool: "Bash".into(), input: json!({}) }, 3);
        assert_eq!(s.list(3)[0].state, SessionState::WaitingApproval);
        s.apply("claude:a", "claude", None, &CoreEvent::SessionEnded, 4);
        assert!(s.list(4).is_empty());
    }

    #[test]
    fn stale_sessions_hidden_and_sorted_newest_first() {
        let mut s = SessionStore::default();
        s.apply("claude:old", "claude", None, &CoreEvent::Idle, 0);
        s.apply("claude:b", "claude", None, &CoreEvent::Idle, 1000);
        s.apply("claude:c", "claude", None, &CoreEvent::Idle, 1500);
        let keys: Vec<_> = s.list(STALE_AFTER_SECS + 1).into_iter().map(|x| x.key).collect();
        assert_eq!(keys, vec!["claude:c", "claude:b"]);
    }

    #[test]
    fn long_prompt_truncated() {
        let mut s = SessionStore::default();
        s.apply("k", "claude", None, &CoreEvent::PromptSubmitted { prompt: "x".repeat(200) }, 1);
        assert!(s.list(1)[0].last_action.as_ref().unwrap().chars().count() <= 61);
    }
}
