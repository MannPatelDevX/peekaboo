use crate::adapters::adapter_for;
use crate::model::*;
use crate::sessions::SessionStore;
use crate::settings::Settings;
use peekaboo_protocol::{HookMessage, HookReply, PROTOCOL_VERSION};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// Called after every state change so the UI can refresh.
pub type Emitter = Arc<dyn Fn() + Send + Sync>;
/// Finds the terminal device of a process (e.g. `ttys004`).
pub type TtyLookup = Arc<dyn Fn(u32) -> Option<String> + Send + Sync>;

/// Walks up from `pid` to the first ancestor with a terminal. Claude Code runs hooks through a
/// shell that has no controlling terminal; the `claude` process above it does.
/// `info(pid)` returns (parent pid, tty).
pub fn walk_tty(pid: u32, info: impl Fn(u32) -> Option<(u32, Option<String>)>) -> Option<String> {
    let mut pid = pid;
    for _ in 0..8 {
        let (parent, tty) = info(pid)?;
        if tty.is_some() {
            return tty;
        }
        if parent <= 1 || parent == pid {
            return None;
        }
        pid = parent;
    }
    None
}

/// `ps -o ppid=,tty=` up the process tree.
pub fn default_tty_lookup() -> TtyLookup {
    Arc::new(|pid| {
        if !cfg!(unix) {
            return None;
        }
        walk_tty(pid, |p| {
            let out = std::process::Command::new("ps").args(["-o", "ppid=,tty=", "-p", &p.to_string()]).output().ok()?;
            let text = String::from_utf8_lossy(&out.stdout);
            let mut parts = text.split_whitespace();
            let parent = parts.next()?.parse().ok()?;
            let tty = parts.next().filter(|t| !t.starts_with('?')).map(str::to_string);
            Some((parent, tty))
        })
    })
}

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
    queued: HashMap<String, VecDeque<String>>,
    tty_tried: HashSet<String>,
}

pub struct Core {
    inner: Mutex<Inner>,
    emit: Emitter,
    settings_path: PathBuf,
    tty_lookup: TtyLookup,
}

impl Core {
    pub fn new(settings_path: PathBuf, emit: Emitter) -> Core {
        let settings = Settings::load(&settings_path);
        Core {
            inner: Mutex::new(Inner {
                sessions: SessionStore::default(),
                pending: HashMap::new(),
                settings,
                queued: HashMap::new(),
                tty_tried: HashSet::new(),
            }),
            emit,
            settings_path,
            tty_lookup: default_tty_lookup(),
        }
    }

    pub fn with_tty_lookup(mut self, lookup: TtyLookup) -> Core {
        self.tty_lookup = lookup;
        self
    }

    /// Queues `prompt` for session `key`; it is sent when the agent next finishes a turn.
    pub fn queue_prompt(&self, key: &str, prompt: &str) -> Result<(), String> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("message is empty".into());
        }
        {
            let mut inner = self.inner.lock().unwrap();
            let Some(session) = inner.sessions.get(key).filter(|s| s.state != SessionState::Ended) else {
                return Err("that session is no longer active".into());
            };
            // A queued prompt is only delivered when the agent finishes a turn; an idle one never will.
            if session.state == SessionState::Idle {
                let app = session.terminal.clone().unwrap_or_else(|| "its terminal".into());
                return Err(format!(
                    "Claude is idle in {app}. Peekaboo can only type into Terminal and iTerm2 — send this from {app}, or resume the session in Terminal from the Sessions tab."
                ));
            }
            inner.queued.entry(key.to_string()).or_default().push_back(prompt.to_string());
        }
        (self.emit)();
        Ok(())
    }

    pub fn session_tty(&self, key: &str) -> Option<String> {
        self.inner.lock().unwrap().sessions.tty(key)
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
        // The tty lookup spawns `ps`; do it once per session and never while holding the lock.
        let first_sighting = msg.pid.is_some() && self.inner.lock().unwrap().tty_tried.insert(key.clone());
        let tty = if first_sighting { msg.pid.and_then(|pid| (self.tty_lookup)(pid)) } else { None };
        let mut result = Handled::Reply(passthrough);
        {
            let mut inner = self.inner.lock().unwrap();
            for ev in adapter.normalize(&msg) {
                inner.sessions.apply(&key, &msg.agent, msg.cwd.as_deref(), &ev, now);
                if let Some(t) = &tty {
                    inner.sessions.set_tty(&key, t.clone());
                }
                if let Some(term) = &msg.term {
                    inner.sessions.set_terminal(&key, terminal_name(term));
                }
                if !msg.expects_reply {
                    continue;
                }
                let kind = match ev {
                    CoreEvent::Idle => {
                        let queued: Vec<String> = inner.queued.remove(&key).map(Vec::from).unwrap_or_default();
                        if !queued.is_empty() {
                            inner.sessions.set_state(&key, SessionState::Working, now);
                            inner.sessions.set_last_action(&key, "Sent your message");
                            let prompt = queued.join("\n\n");
                            result = Handled::Reply(self.render_reply(&msg, &Decision::Continue { prompt }));
                        }
                        continue;
                    }
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
        let mut sessions = inner.sessions.list(now_secs());
        for s in &mut sessions {
            s.queued = inner.queued.get(&s.key).map_or(0, VecDeque::len);
        }
        AppState { sessions, pending, rules: inner.settings.rules.clone(), hooks: vec![] }
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
            payload: json!({"session_id": session, "tool_name": tool, "tool_input": {}}), expects_reply: true, term: None }
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

    fn stop(id: &str, session: &str) -> HookMessage {
        HookMessage { v: 1, id: id.into(), agent: "claude".into(), event: "Stop".into(), cwd: Some("/w/app".into()),
            pid: Some(42), payload: json!({"session_id": session}), expects_reply: true, term: None }
    }

    #[test]
    fn queued_prompt_is_delivered_when_claude_stops() {
        let (_d, c, _) = core();
        let Handled::Reply(r) = c.handle(stop("1", "s")) else { panic!() };
        assert!(r.stdout.is_empty(), "nothing queued: let Claude stop");
        let mut working = stop("w", "s");
        working.event = "UserPromptSubmit".into();
        working.expects_reply = false;
        c.handle(working);
        c.queue_prompt("claude:s", "now add tests").unwrap();
        assert_eq!(c.snapshot().sessions[0].queued, 1);
        let Handled::Reply(r) = c.handle(stop("2", "s")) else { panic!() };
        let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
        assert!(v["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("now add tests"));
        let snap = c.snapshot();
        assert_eq!(snap.sessions[0].queued, 0);
        assert_eq!(snap.sessions[0].state, SessionState::Working);
        let Handled::Reply(r) = c.handle(stop("3", "s")) else { panic!() };
        assert!(r.stdout.is_empty(), "delivered once");
    }

    #[test]
    fn queue_needs_a_known_session_and_text() {
        let (_d, c, _) = core();
        assert!(c.queue_prompt("claude:nope", "hi").is_err());
        c.handle(stop("1", "s"));
        assert!(c.queue_prompt("claude:s", "   ").is_err());
    }

    #[test]
    fn records_session_tty_once() {
        let d = tempfile::tempdir().unwrap();
        let lookups = Arc::new(AtomicUsize::new(0));
        let l2 = lookups.clone();
        let c = Core::new(d.path().join("settings.json"), Arc::new(|| {})).with_tty_lookup(Arc::new(move |pid| {
            l2.fetch_add(1, Ordering::SeqCst);
            (pid == 42).then(|| "ttys004".to_string())
        }));
        c.handle(stop("1", "s"));
        c.handle(stop("2", "s"));
        assert_eq!(c.snapshot().sessions[0].tty.as_deref(), Some("ttys004"));
        assert_eq!(c.session_tty("claude:s").as_deref(), Some("ttys004"));
        assert_eq!(lookups.load(Ordering::SeqCst), 1, "looked up once per session");
    }

    #[test]
    fn tty_walks_up_past_detached_hook_shells() {
        // hook's parent: zsh with no tty (pid 30) → claude on ttys002 (pid 20)
        let table = |pid: u32| match pid {
            30 => Some((20, None)),
            20 => Some((10, Some("ttys002".to_string()))),
            _ => None,
        };
        assert_eq!(walk_tty(30, table).as_deref(), Some("ttys002"));
        assert_eq!(walk_tty(99, table), None);
        let looping = |pid: u32| Some((pid, None));
        assert_eq!(walk_tty(5, looping), None, "terminates on cycles");
    }

    #[test]
    fn records_terminal_app_from_hook() {
        let (_d, c, _) = core();
        let mut m = stop("1", "s");
        m.term = Some("WarpTerminal".into());
        c.handle(m);
        assert_eq!(c.snapshot().sessions[0].terminal.as_deref(), Some("Warp"));
    }

    #[test]
    fn idle_sessions_are_not_silently_queued() {
        let (_d, c, _) = core();
        c.handle(stop("1", "s")); // Stop → idle
        let err = c.queue_prompt("claude:s", "hi").unwrap_err();
        assert!(err.contains("idle"), "{err}");
        let mut prompt = stop("2", "s");
        prompt.event = "UserPromptSubmit".into();
        prompt.expects_reply = false;
        c.handle(prompt); // working again
        assert!(c.queue_prompt("claude:s", "hi").is_ok());
    }
}
