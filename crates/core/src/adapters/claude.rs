//! Claude Code hooks: https://code.claude.com/docs/en/hooks
use super::{str_field, AgentAdapter};
use crate::model::{Answer, CoreEvent, Decision, Question};
use peekaboo_protocol::HookMessage;
use serde_json::{json, Map, Value};

pub struct ClaudeAdapter;

const DENY_MESSAGE: &str = "Denied from Peekaboo";

fn tool_input(msg: &HookMessage) -> Value {
    msg.payload.get("tool_input").cloned().unwrap_or(Value::Null)
}

fn parse_questions(input: &Value) -> Vec<Question> {
    input.get("questions").cloned().and_then(|q| serde_json::from_value(q).ok()).unwrap_or_default()
}

/// AskUserQuestion is answered by allowing the call with the original input plus an
/// `answers` map (question text → chosen label), as documented for PreToolUse hooks.
fn answered_input(msg: &HookMessage, answers: &[Answer]) -> Value {
    let mut input = match tool_input(msg) {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    let map: Map<String, Value> = answers.iter().map(|a| (a.question.clone(), json!(a.answer))).collect();
    input.insert("answers".into(), Value::Object(map));
    Value::Object(input)
}

impl AgentAdapter for ClaudeAdapter {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn session_key(&self, msg: &HookMessage) -> String {
        match msg.payload.get("session_id").and_then(Value::as_str) {
            Some(id) => format!("claude:{id}"),
            None => format!("claude:{}:{}", msg.pid.unwrap_or(0), msg.cwd.clone().unwrap_or_default()),
        }
    }

    fn normalize(&self, msg: &HookMessage) -> Vec<CoreEvent> {
        let p = &msg.payload;
        let tool = str_field(p, "tool_name");
        let input = tool_input(msg);
        match msg.event.as_str() {
            "SessionStart" => vec![CoreEvent::SessionStarted],
            "SessionEnd" => vec![CoreEvent::SessionEnded],
            "UserPromptSubmit" => vec![CoreEvent::PromptSubmitted { prompt: str_field(p, "prompt") }],
            "PreToolUse" if tool == "AskUserQuestion" => vec![CoreEvent::QuestionAsked { questions: parse_questions(&input) }],
            "PreToolUse" => vec![CoreEvent::ToolStarted { tool, input }],
            "PostToolUse" => vec![CoreEvent::ToolFinished { tool }],
            "PermissionRequest" if tool == "AskUserQuestion" => vec![],
            "PermissionRequest" => vec![CoreEvent::PermissionRequested { tool, input }],
            "Notification" => vec![CoreEvent::Notification { text: str_field(p, "message") }],
            "Stop" => vec![CoreEvent::Idle],
            _ => vec![],
        }
    }

    fn render_reply(&self, msg: &HookMessage, decision: &Decision) -> (String, i32) {
        let out = match (msg.event.as_str(), decision) {
            ("PermissionRequest", Decision::Allow { .. }) => json!({"hookSpecificOutput": {
                "hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}}),
            ("PermissionRequest", Decision::Deny { reason }) => json!({"hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": {"behavior": "deny", "message": reason.clone().unwrap_or_else(|| DENY_MESSAGE.into())}}}),
            ("PreToolUse", Decision::Answer { answers }) => json!({"hookSpecificOutput": {
                "hookEventName": "PreToolUse", "permissionDecision": "allow",
                "updatedInput": answered_input(msg, answers)}}),
            ("PreToolUse", Decision::Deny { reason }) => json!({"hookSpecificOutput": {
                "hookEventName": "PreToolUse", "permissionDecision": "deny",
                "permissionDecisionReason": reason.clone().unwrap_or_else(|| DENY_MESSAGE.into())}}),
            ("Stop", Decision::Continue { prompt }) => json!({"hookSpecificOutput": {
                "hookEventName": "Stop",
                "additionalContext": format!("The user sent this message from Peekaboo. Treat it as their next message:\n\n{prompt}")}}),
            _ => return (String::new(), 0),
        };
        (out.to_string(), 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use peekaboo_protocol::HookMessage;
    use serde_json::json;

    fn msg(event: &str, payload: serde_json::Value) -> HookMessage {
        HookMessage { v: 1, id: "id1".into(), agent: "claude".into(), event: event.into(),
            cwd: Some("/work/shop".into()), pid: Some(9), payload, expects_reply: true, term: None }
    }

    fn ask_payload() -> serde_json::Value {
        json!({"tool_name":"AskUserQuestion","tool_input":{"questions":[
            {"question":"Which DB?","header":"DB","multiSelect":false,"options":[{"label":"Postgres","description":"SQL"},{"label":"Mongo"}]}
        ]}})
    }

    #[test]
    fn session_key_uses_session_id() {
        assert_eq!(ClaudeAdapter.session_key(&msg("Stop", json!({"session_id": "abc"}))), "claude:abc");
        assert_eq!(ClaudeAdapter.session_key(&msg("Stop", json!({}))), "claude:9:/work/shop");
    }

    #[test]
    fn normalizes_permission_request() {
        let m = msg("PermissionRequest", json!({"session_id":"s","tool_name":"Bash","tool_input":{"command":"ls"}}));
        assert_eq!(ClaudeAdapter.normalize(&m), vec![CoreEvent::PermissionRequested { tool: "Bash".into(), input: json!({"command":"ls"}) }]);
    }

    #[test]
    fn normalizes_ask_user_question() {
        let ev = ClaudeAdapter.normalize(&msg("PreToolUse", ask_payload()));
        let CoreEvent::QuestionAsked { questions } = &ev[0] else { panic!("{ev:?}") };
        assert_eq!(questions[0].question, "Which DB?");
        assert_eq!(questions[0].options[1].label, "Mongo");
    }

    #[test]
    fn normalizes_lifecycle() {
        assert_eq!(ClaudeAdapter.normalize(&msg("Stop", json!({}))), vec![CoreEvent::Idle]);
        assert_eq!(ClaudeAdapter.normalize(&msg("SessionEnd", json!({}))), vec![CoreEvent::SessionEnded]);
        assert_eq!(ClaudeAdapter.normalize(&msg("UserPromptSubmit", json!({"prompt":"hi"}))), vec![CoreEvent::PromptSubmitted { prompt: "hi".into() }]);
        assert_eq!(ClaudeAdapter.normalize(&msg("Weird", json!({}))), vec![]);
    }

    #[test]
    fn renders_permission_allow_and_deny() {
        let m = msg("PermissionRequest", json!({}));
        let (out, code) = ClaudeAdapter.render_reply(&m, &Decision::Allow { always: false });
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(code, 0);
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PermissionRequest");
        assert_eq!(v["hookSpecificOutput"]["decision"]["behavior"], "allow");
        let (out, _) = ClaudeAdapter.render_reply(&m, &Decision::Deny { reason: None });
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert_eq!(v["hookSpecificOutput"]["decision"]["message"], "Denied from Peekaboo");
    }

    #[test]
    fn renders_answers_as_allow_with_updated_input() {
        let m = msg("PreToolUse", ask_payload());
        let d = Decision::Answer { answers: vec![Answer { question: "Which DB?".into(), answer: "Postgres".into() }] };
        let (out, _) = ClaudeAdapter.render_reply(&m, &d);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let h = &v["hookSpecificOutput"];
        assert_eq!(h["hookEventName"], "PreToolUse");
        assert_eq!(h["permissionDecision"], "allow");
        assert_eq!(h["updatedInput"]["answers"]["Which DB?"], "Postgres");
        assert_eq!(h["updatedInput"]["questions"], ask_payload()["tool_input"]["questions"]);
    }

    #[test]
    fn passthrough_renders_nothing() {
        assert_eq!(ClaudeAdapter.render_reply(&msg("PermissionRequest", json!({})), &Decision::Passthrough), (String::new(), 0));
    }

    #[test]
    fn permission_request_for_ask_user_question_is_ignored() {
        // Questions are answered through PreToolUse; a second card here would carry no answers.
        let m = msg("PermissionRequest", json!({"tool_name":"AskUserQuestion","tool_input":{}}));
        assert_eq!(ClaudeAdapter.normalize(&m), vec![]);
    }

    #[test]
    fn continue_renders_stop_feedback() {
        let (out, code) = ClaudeAdapter.render_reply(&msg("Stop", json!({})), &Decision::Continue { prompt: "add tests".into() });
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(code, 0);
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "Stop");
        assert!(v["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("add tests"));
    }
}
