#![cfg(unix)]
use peekaboo_core::{ipc, model::Decision, Core};
use peekaboo_protocol::SOCKET_ENV;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

fn spawn_hook(sock: &std::path::Path, event: &str, payload: &str) -> Child {
    let mut c = Command::new(env!("CARGO_BIN_EXE_peekaboo-hook"))
        .args(["--agent", "claude", "--event", event, "--wait"])
        .env(SOCKET_ENV, sock)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    c
}

async fn setup() -> (tempfile::TempDir, std::path::PathBuf, Arc<Core>) {
    let d = tempfile::tempdir().unwrap();
    let sock = d.path().join("pb.sock");
    let core = Arc::new(Core::new(d.path().join("settings.json"), Arc::new(|| {})));
    let (c2, s2) = (core.clone(), sock.clone());
    tokio::spawn(async move { ipc::serve_unix(c2, &s2).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(100)).await;
    (d, sock, core)
}

async fn wait_pending(core: &Core, n: usize) {
    for _ in 0..100 {
        if core.snapshot().pending.len() == n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("pending never reached {n}");
}

const PERM: &str = r#"{"session_id":"s1","tool_name":"Bash","tool_input":{"command":"rm -rf build"}}"#;
const ASK: &str = r#"{"session_id":"s1","tool_name":"AskUserQuestion","tool_input":{"questions":[{"question":"Which DB?","header":"DB","multiSelect":false,"options":[{"label":"Postgres"},{"label":"Mongo"}]}]}}"#;

#[tokio::test(flavor = "multi_thread")]
async fn approve_from_ui_reaches_agent() {
    let (_d, sock, core) = setup().await;
    let child = spawn_hook(&sock, "PermissionRequest", PERM);
    wait_pending(&core, 1).await;
    let id = core.snapshot().pending[0].id.clone();
    core.decide(&id, Decision::Allow { always: false }).unwrap();
    let out = tokio::task::spawn_blocking(move || child.wait_with_output().unwrap()).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["hookSpecificOutput"]["decision"]["behavior"], "allow");
    assert_eq!(out.status.code(), Some(0));
}

#[tokio::test(flavor = "multi_thread")]
async fn answer_from_ui_reaches_agent() {
    let (_d, sock, core) = setup().await;
    let child = spawn_hook(&sock, "PreToolUse", ASK);
    wait_pending(&core, 1).await;
    let id = core.snapshot().pending[0].id.clone();
    let answers = vec![peekaboo_core::model::Answer { question: "Which DB?".into(), answer: "Mongo".into() }];
    core.decide(&id, Decision::Answer { answers }).unwrap();
    let out = tokio::task::spawn_blocking(move || child.wait_with_output().unwrap()).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["hookSpecificOutput"]["updatedInput"]["answers"]["Which DB?"], "Mongo");
}

#[tokio::test(flavor = "multi_thread")]
async fn killed_hook_clears_pending() {
    let (_d, sock, core) = setup().await;
    let mut child = spawn_hook(&sock, "PermissionRequest", PERM);
    wait_pending(&core, 1).await;
    child.kill().unwrap();
    let _ = child.wait();
    wait_pending(&core, 0).await;
    assert_ne!(core.snapshot().sessions[0].state, peekaboo_core::model::SessionState::WaitingApproval);
}

#[tokio::test(flavor = "multi_thread")]
async fn wake_hook_exits_2_with_the_prompt_to_wake_claude() {
    let (_d, sock, core) = setup().await;
    let child = spawn_hook(&sock, "Wake", r#"{"session_id":"s1"}"#);
    for _ in 0..100 {
        if core.snapshot().sessions.first().is_some_and(|s| s.reachable) { break }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(core.snapshot().sessions[0].reachable, "waiter registered");
    // The waiter arrives before any Stop in this test, so the session isn't idle yet: the prompt
    // still goes straight to the waiter.
    assert_eq!(core.queue_prompt("claude:s1", "now write docs").unwrap(), peekaboo_core::Delivery::Delivered);
    let out = tokio::task::spawn_blocking(move || child.wait_with_output().unwrap()).await.unwrap();
    assert_eq!(out.status.code(), Some(2), "exit 2 wakes Claude");
    assert!(String::from_utf8_lossy(&out.stdout).contains("now write docs"));
}
