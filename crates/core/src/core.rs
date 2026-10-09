use crate::adapters::adapter_for;
use crate::model::*;
use crate::sessions::SessionStore;
use crate::settings::Settings;
use peekaboo_protocol::{HookMessage, HookReply, PROTOCOL_VERSION};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// Called after every state change so the UI can refresh.
pub type Emitter = Arc<dyn Fn() + Send + Sync>;

pub enum Handled {
    /// Answer the hook now.
    Reply(HookReply),
    /// Hold the connection until the user decides (or the hook goes away).
    Wait { msg: HookMessage, rx: oneshot::Receiver<Decision> },
}

struct Pending {
    view: PendingView,
    cwd: Option<String>,
    tx: oneshot::Sender<Decision>,
}

struct Inner {
    sessions: SessionStore,
    pending: HashMap<String, Pending>,
    settings: Settings,
}

pub struct Core {
    inner: Mutex<Inner>,
    emit: Emitter,
    settings_path: PathBuf,
}

impl Core {
    pub fn new(settings_path: PathBuf, emit: Emitter) -> Core {
        let settings = Settings::load(&settings_path);
        Core {
            inner: Mutex::new(Inner { sessions: SessionStore::default(), pending: HashMap::new(), settings }),
            emit,
            settings_path,
        }
    }

    pub fn approval_timeout_secs(&self) -> u64 {
        self.inner.lock().unwrap().settings.approval_timeout_secs
    }

    pub fn handle(&self, msg: HookMessage) -> Handled {
        let passthrough = HookReply::passthrough(&msg.id);
        if msg.v != PROTOCOL_VERSION {
            return Handled::Reply(passthrough);
        }
        let Some(adapter) = adapter_for(&msg.agent) else { return Handled::Reply(passthrough) };
        let key = adapter.session_key(&msg);
        let now = now_secs();
        let mut result = Handled::Reply(passthrough);
        {
            let mut inner = self.inner.lock().unwrap();
            for ev in adapter.normalize(&msg) {
                inner.sessions.apply(&key, &msg.agent, msg.cwd.as_deref(), &ev, now);
                if !msg.expects_reply {
                    continue;
                }
                let kind = match ev {
                    CoreEvent::PermissionRequested { tool, input } => {
                        if inner.settings.allows(&msg.agent, &tool, &input, msg.cwd.as_deref()) {
                            inner.sessions.set_state(&key, SessionState::Working, now);
                            result = Handled::Reply(self.render_reply(&msg, &Decision::Allow { always: false }));
                            continue;
                        }
                        PendingKind::Permission { tool, input }
                    }
                    CoreEvent::QuestionAsked { questions } => PendingKind::Question { questions },
                    _ => continue,
                };
                let (tx, rx) = oneshot::channel();
                let view = PendingView {
                    id: msg.id.clone(),
                    agent: msg.agent.clone(),
                    session_key: key.clone(),
                    project: project_name(msg.cwd.as_deref()),
                    kind,
                    created_at: now,
                };
                inner.pending.insert(msg.id.clone(), Pending { view, cwd: msg.cwd.clone(), tx });
                result = Handled::Wait { msg: msg.clone(), rx };
            }
        }
        (self.emit)();
        result
    }

    pub fn decide(&self, id: &str, decision: Decision) -> Result<(), String> {
        let pending = {
            let mut inner = self.inner.lock().unwrap();
            let p = inner.pending.remove(id).ok_or_else(|| format!("no pending request {id}"))?;
            if let (Decision::Allow { always: true }, PendingKind::Permission { tool, input }) = (&decision, &p.view.kind) {
                inner.settings.add_rule(AlwaysRule::for_request(&p.view.agent, tool, input, p.cwd.as_deref()));
                inner.settings.save(&self.settings_path).map_err(|e| e.to_string())?;
            }
            inner.sessions.set_state(&p.view.session_key, SessionState::Working, now_secs());
            p
        };
        let _ = pending.tx.send(decision);
        (self.emit)();
        Ok(())
    }

    /// The hook disconnected (agent killed or hook timed out).
    pub fn cancel(&self, id: &str) {
        let removed = {
            let mut inner = self.inner.lock().unwrap();
            let p = inner.pending.remove(id);
            if let Some(p) = &p {
                inner.sessions.set_state(&p.view.session_key, SessionState::Working, now_secs());
            }
            p.is_some()
        };
        if removed {
            (self.emit)();
        }
    }

    pub fn render_reply(&self, msg: &HookMessage, decision: &Decision) -> HookReply {
        let (stdout, exit_code) = adapter_for(&msg.agent).map(|a| a.render_reply(msg, decision)).unwrap_or_default();
        HookReply { v: PROTOCOL_VERSION, id: msg.id.clone(), stdout, exit_code }
    }

    pub fn remove_rule(&self, rule: &AlwaysRule) -> Result<(), String> {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.settings.remove_rule(rule);
            inner.settings.save(&self.settings_path).map_err(|e| e.to_string())?;
        }
        (self.emit)();
        Ok(())
    }

    pub fn snapshot(&self) -> AppState {
        let inner = self.inner.lock().unwrap();
        let mut pending: Vec<PendingView> = inner.pending.values().map(|p| p.view.clone()).collect();
        pending.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
        AppState { sessions: inner.sessions.list(now_secs()), pending, rules: inner.settings.rules.clone(), hooks: vec![] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn core() -> (tempfile::TempDir, Core, Arc<AtomicUsize>) {
        let d = tempfile::tempdir().unwrap();
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let c = Core::new(d.path().join("settings.json"), Arc::new(move || { n2.fetch_add(1, Ordering::SeqCst); }));
        (d, c, n)
    }

    fn perm(id: &str, session: &str, tool: &str) -> HookMessage {
        HookMessage { v: 1, id: id.into(), agent: "claude".into(), event: "PermissionRequest".into(),
            cwd: Some("/w/app".into()), pid: None,
            payload: json!({"session_id": session, "tool_name": tool, "tool_input": {}}), expects_reply: true }
    }

    #[test]
    fn unknown_agent_or_version_passes_through() {
        let (_d, c, _) = core();
        let mut m = perm("1", "s", "Bash");
        m.agent = "nope".into();
        assert!(matches!(c.handle(m), Handled::Reply(r) if r.stdout.is_empty()));
        let mut m = perm("2", "s", "Bash");
        m.v = 99;
        assert!(matches!(c.handle(m), Handled::Reply(r) if r.stdout.is_empty()));
    }

    #[test]
    fn notification_updates_session_and_emits() {
        let (_d, c, n) = core();
        let mut m = perm("1", "s", "Bash");
        m.event = "Stop".into();
        m.expects_reply = false;
        assert!(matches!(c.handle(m), Handled::Reply(_)));
        assert_eq!(c.snapshot().sessions.len(), 1);
        assert!(n.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn permission_waits_then_decide_resolves() {
        let (_d, c, _) = core();
        let Handled::Wait { msg, mut rx } = c.handle(perm("1", "s", "Bash")) else { panic!() };
        assert_eq!(c.snapshot().pending.len(), 1);
        c.decide("1", Decision::Allow { always: false }).unwrap();
        let d = rx.try_recv().unwrap();
        assert!(c.snapshot().pending.is_empty());
        assert!(c.render_reply(&msg, &d).stdout.contains("allow"));
        assert_eq!(c.snapshot().sessions[0].state, SessionState::Working);
    }

    #[test]
    fn two_pending_resolve_independently() {
        let (_d, c, _) = core();
        let Handled::Wait { rx: mut rx1, .. } = c.handle(perm("1", "s1", "Bash")) else { panic!() };
        let Handled::Wait { rx: mut rx2, .. } = c.handle(perm("2", "s2", "Edit")) else { panic!() };
        c.decide("2", Decision::Deny { reason: None }).unwrap();
        assert!(rx1.try_recv().is_err());
        assert_eq!(rx2.try_recv().unwrap(), Decision::Deny { reason: None });
        assert_eq!(c.snapshot().pending.len(), 1);
        assert_eq!(c.snapshot().pending[0].id, "1");
    }

    fn bash(id: &str, cwd: &str, command: &str) -> HookMessage {
        let mut m = perm(id, "s", "Bash");
        m.cwd = Some(cwd.into());
        m.payload["tool_input"] = json!({ "command": command });
        m
    }

    #[test]
    fn always_creates_scoped_rule_and_auto_allows_only_that() {
        let (d, c, _) = core();
        let Handled::Wait { .. } = c.handle(bash("1", "/w/app", "npm test")) else { panic!() };
        c.decide("1", Decision::Allow { always: true }).unwrap();
        assert_eq!(c.snapshot().rules, vec![AlwaysRule {
            agent: "claude".into(), tool: "Bash".into(), cwd: Some("/w/app".into()), command: Some("npm test".into()),
        }]);
        match c.handle(bash("2", "/w/app", "npm test")) {
            Handled::Reply(r) => assert!(r.stdout.contains("allow")),
            _ => panic!("expected auto-allow"),
        }
        assert!(matches!(c.handle(bash("3", "/w/app", "rm -rf ~")), Handled::Wait { .. }));
        assert!(matches!(c.handle(bash("4", "/w/other", "npm test")), Handled::Wait { .. }));
        let saved = Settings::load(&d.path().join("settings.json"));
        assert!(saved.allows("claude", "Bash", &json!({"command": "npm test"}), Some("/w/app")));
    }

    #[test]
    fn cancel_clears_pending_and_waiting_state() {
        let (_d, c, _) = core();
        let Handled::Wait { .. } = c.handle(perm("1", "s", "Bash")) else { panic!() };
        c.cancel("1");
        let snap = c.snapshot();
        assert!(snap.pending.is_empty());
        assert_eq!(snap.sessions[0].state, SessionState::Working);
    }

    #[test]
    fn decide_unknown_id_errors() {
        let (_d, c, _) = core();
        assert!(c.decide("nope", Decision::Passthrough).is_err());
    }
}
