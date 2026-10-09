# Peekaboo Phase 1 (Core) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A cross-platform Tauri app that tracks Claude Code sessions and lets the user approve permissions and answer `AskUserQuestion` prompts from a notch/top-edge overlay, without ever blocking the agent.

**Architecture:** A tiny agent-agnostic `peekaboo-hook` binary forwards hook payloads as length-prefixed JSON over a Unix socket (named pipe on Windows) to a Rust core library (`peekaboo-core`, no Tauri dependency, fully unit-tested). Per-agent adapters normalize events and render replies. A thin Tauri shell hosts the core, exposes commands, and renders a React overlay.

**Tech Stack:** Rust (stable), Tauri 2, tokio, serde/serde_json, thiserror, uuid, dirs; TypeScript, React, Vite, Vitest + Testing Library.

**Spec:** `docs/superpowers/specs/2026-10-09-peekaboo-design.md`

## Global Constraints

- Product name `Peekaboo`, mascot `Boo`, bundle identifier `ai.devxlabs.peekaboo`. No Coucou names, characters, sounds, or art anywhere.
- Hook protocol version `v: 1`; framing = 4-byte big-endian length + UTF-8 JSON; max frame 8 MiB.
- Every hook failure path (no socket, refused, timeout, malformed/mismatched reply) → exit code 0 with empty stdout.
- Default approval timeout 600 s; installed Claude hook `timeout` must be approval timeout + 10 s.
- Unix socket: macOS `~/Library/Application Support/Peekaboo/peekaboo.sock`, Linux `$XDG_RUNTIME_DIR/peekaboo.sock`, file mode `0600`. Windows pipe `\\.\pipe\peekaboo`. Env `PEEKABOO_SOCKET` overrides both (used by tests).
- Installer never writes an agent config without a timestamped backup when the file exists, and refuses to touch a config that is not valid JSON object.
- No telemetry, no network calls in Phase 1.
- Global shortcut `CommandOrControl+Alt+P` toggles the overlay.
- Deviation from spec §2 layout (intentional): core logic lives in `crates/core` (no Tauri dep) so it is testable without the app; `src-tauri` is a thin shell.

## Review Focus

1. Two agent sessions request permission at the same time → each card resolves only its own hook; the other stays pending. (Task 5 `two_pending_resolve_independently`)
2. User kills the agent (or the hook times out) while an approval card is showing → the card disappears and the session leaves `WaitingApproval`. (Task 6 `killed_hook_clears_pending`)
3. User already has their own hooks in `~/.claude/settings.json` → install and uninstall leave them byte-for-byte equivalent in JSON. (Task 7 `preserves_user_hooks`)
4. Something writes garbage to the socket → that connection is dropped and the next valid hook still works. (Task 6 `garbage_frame_does_not_kill_server`)
5. App crashed last time and left a stale socket file → next launch binds successfully. (Task 6 `serve_replaces_stale_socket_file`)

---

## File Structure

```
peekaboo/
  Cargo.toml                     # workspace; default-members = crates/*
  package.json, vite.config.ts, tsconfig.json, index.html
  crates/
    protocol/src/lib.rs          # HookMessage/HookReply, framing, socket path
    hook/src/main.rs             # peekaboo-hook binary
    hook/tests/hook.rs           # hook vs fake server (unix)
    hook/tests/e2e.rs            # hook vs real core IPC server (unix)
    core/src/lib.rs              # module exports
    core/src/model.rs            # CoreEvent, Decision, Session, PendingView, AppState…
    core/src/adapters/mod.rs     # AgentAdapter trait + registry
    core/src/adapters/claude.rs  # Claude Code adapter
    core/src/sessions.rs         # SessionStore
    core/src/settings.rs         # Settings + Always rules persistence
    core/src/core.rs             # Core orchestration (handle/decide/cancel/snapshot)
    core/src/ipc.rs              # socket / pipe server
    core/src/installer/mod.rs    # HookInstaller trait, shared helpers
    core/src/installer/claude.rs # ~/.claude/settings.json installer
  src-tauri/                     # Tauri shell: lib.rs, commands.rs, overlay.rs
  src/                           # React overlay
  scripts/prepare-sidecar.mjs
  .github/workflows/{ci,release}.yml
```

---

### Task 1: Toolchain, workspace, protocol crate

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `crates/protocol/Cargo.toml`, `crates/protocol/src/lib.rs`

**Interfaces:**
- Produces: `peekaboo_protocol::{PROTOCOL_VERSION, MAX_FRAME_BYTES, SOCKET_ENV, HookMessage, HookReply, FrameError, encode_frame, decode_len, write_frame, read_frame}`; `socket_path() -> PathBuf` (unix); `pipe_name() -> String` (windows).

- [ ] **Step 1: Install Rust** (ask user first): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y` then `source ~/.cargo/env && cargo --version`.

- [ ] **Step 2: Workspace files**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/*", "src-tauri"]
default-members = ["crates/protocol", "crates/hook", "crates/core"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "UNLICENSED"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
uuid = { version = "1", features = ["v4"] }
dirs = "6"
tokio = { version = "1", features = ["rt-multi-thread", "net", "io-util", "sync", "macros", "time"] }
tempfile = "3"
```
(`src-tauri` is added to `members` in Task 8; until then remove it from the list.)

`.gitignore`:
```
target/
node_modules/
dist/
src-tauri/binaries/
src-tauri/gen/
.DS_Store
```

`crates/protocol/Cargo.toml`:
```toml
[package]
name = "peekaboo-protocol"
version.workspace = true
edition.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
dirs.workspace = true
```

- [ ] **Step 3: Write failing tests** at the bottom of `crates/protocol/src/lib.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn msg() -> HookMessage {
        HookMessage {
            v: PROTOCOL_VERSION, id: "abc".into(), agent: "claude".into(),
            event: "PermissionRequest".into(), cwd: Some("/tmp/x".into()), pid: Some(1),
            payload: serde_json::json!({"tool_name": "Bash"}), expects_reply: true,
        }
    }

    #[test]
    fn frame_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg()).unwrap();
        let back: HookMessage = read_frame(&mut Cursor::new(buf)).unwrap();
        assert_eq!(back, msg());
    }

    #[test]
    fn rejects_oversized_header() {
        let header = (MAX_FRAME_BYTES + 1).to_be_bytes();
        assert!(matches!(decode_len(header), Err(FrameError::TooLarge(_))));
    }

    #[test]
    fn truncated_frame_is_io_error() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg()).unwrap();
        buf.truncate(buf.len() - 3);
        let r: Result<HookMessage, _> = read_frame(&mut Cursor::new(buf));
        assert!(matches!(r, Err(FrameError::Io(_))));
    }

    #[test]
    fn passthrough_reply_is_empty_success() {
        let r = HookReply::passthrough("abc");
        assert_eq!((r.v, r.id.as_str(), r.stdout.as_str(), r.exit_code), (1, "abc", "", 0));
    }

    #[cfg(unix)]
    #[test]
    fn socket_path_respects_env() {
        std::env::set_var(SOCKET_ENV, "/tmp/pb-test.sock");
        assert_eq!(socket_path(), std::path::PathBuf::from("/tmp/pb-test.sock"));
        std::env::remove_var(SOCKET_ENV);
    }
}
```

- [ ] **Step 4: Run** `cargo test -p peekaboo-protocol` → expect compile FAIL (types missing).

- [ ] **Step 5: Implement** above the tests in `crates/protocol/src/lib.rs`:
```rust
//! Wire protocol shared by `peekaboo-hook` and the Peekaboo core.
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: u32 = 8 * 1024 * 1024;
/// Overrides the socket path / pipe name. Used by tests.
pub const SOCKET_ENV: &str = "PEEKABOO_SOCKET";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookMessage {
    pub v: u32,
    pub id: String,
    pub agent: String,
    pub event: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub pid: Option<u32>,
    pub payload: serde_json::Value,
    pub expects_reply: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookReply {
    pub v: u32,
    pub id: String,
    pub stdout: String,
    pub exit_code: i32,
}

impl HookReply {
    /// A reply that makes the agent behave as if no hook ran.
    pub fn passthrough(id: &str) -> Self {
        Self { v: PROTOCOL_VERSION, id: id.to_string(), stdout: String::new(), exit_code: 0 }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("frame too large: {0} bytes")]
    TooLarge(u32),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn encode_frame<T: Serialize>(msg: &T) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(msg)?;
    let len = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge(u32::MAX))?;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

pub fn decode_len(header: [u8; 4]) -> Result<usize, FrameError> {
    let len = u32::from_be_bytes(header);
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    Ok(len as usize)
}

pub fn write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> Result<(), FrameError> {
    w.write_all(&encode_frame(msg)?)?;
    w.flush()?;
    Ok(())
}

pub fn read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> Result<T, FrameError> {
    let mut header = [0u8; 4];
    r.read_exact(&mut header)?;
    let mut body = vec![0u8; decode_len(header)?];
    r.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

#[cfg(unix)]
pub fn socket_path() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os(SOCKET_ENV) {
        return p.into();
    }
    #[cfg(target_os = "macos")]
    {
        dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("Peekaboo").join("peekaboo.sock")
    }
    #[cfg(not(target_os = "macos"))]
    {
        match dirs::runtime_dir() {
            Some(d) => d.join("peekaboo.sock"),
            None => std::env::temp_dir()
                .join(format!("peekaboo-{}.sock", std::env::var("USER").unwrap_or_default())),
        }
    }
}

#[cfg(windows)]
pub fn pipe_name() -> String {
    std::env::var(SOCKET_ENV).unwrap_or_else(|_| r"\\.\pipe\peekaboo".to_string())
}
```

- [ ] **Step 6: Run** `cargo test -p peekaboo-protocol` → PASS (5 tests).

- [ ] **Step 7: Commit** `git add -A && git commit -m "feat(protocol): hook wire protocol and framing"`

---

### Task 2: `peekaboo-hook` binary

**Files:**
- Create: `crates/hook/Cargo.toml`, `crates/hook/src/main.rs`, `crates/hook/tests/hook.rs`

**Interfaces:**
- Consumes: Task 1 protocol API.
- Produces: binary `peekaboo-hook --agent <a> --event <e> [--wait] [--timeout-secs N]`, reads payload JSON on stdin, prints reply `stdout`, exits with reply `exit_code` (0 on every failure).

- [ ] **Step 1: Cargo.toml**
```toml
[package]
name = "peekaboo-hook"
version.workspace = true
edition.workspace = true

[[bin]]
name = "peekaboo-hook"
path = "src/main.rs"

[dependencies]
peekaboo-protocol = { path = "../protocol" }
serde_json.workspace = true
uuid.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Step 2: Write failing tests** `crates/hook/tests/hook.rs`:
```rust
#![cfg(unix)]
use peekaboo_protocol::{read_frame, write_frame, HookMessage, HookReply, SOCKET_ENV};
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn run_hook(sock: &Path, args: &[&str], stdin: &str) -> (String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_peekaboo-hook"))
        .args(args)
        .env(SOCKET_ENV, sock)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    (String::from_utf8(out.stdout).unwrap(), out.status.code().unwrap())
}

fn sock() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("pb.sock");
    (dir, p)
}

#[test]
fn no_server_exits_zero_silently() {
    let (_d, p) = sock();
    let started = Instant::now();
    assert_eq!(run_hook(&p, &["--agent", "claude", "--event", "Stop", "--wait"], "{}"), (String::new(), 0));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn notify_sends_message_and_does_not_wait() {
    let (_d, p) = sock();
    let listener = UnixListener::bind(&p).unwrap();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let m: HookMessage = read_frame(&mut s).unwrap();
        m
    });
    let out = run_hook(&p, &["--agent", "claude", "--event", "Stop"], r#"{"session_id":"s1"}"#);
    assert_eq!(out, (String::new(), 0));
    let m = server.join().unwrap();
    assert_eq!((m.agent.as_str(), m.event.as_str(), m.expects_reply), ("claude", "Stop", false));
    assert_eq!(m.payload["session_id"], "s1");
}

#[test]
fn wait_prints_server_stdout() {
    let (_d, p) = sock();
    let listener = UnixListener::bind(&p).unwrap();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let m: HookMessage = read_frame(&mut s).unwrap();
        write_frame(&mut s, &HookReply { v: 1, id: m.id, stdout: "{\"ok\":true}".into(), exit_code: 0 }).unwrap();
    });
    let out = run_hook(&p, &["--agent", "claude", "--event", "PermissionRequest", "--wait"], "{}");
    assert_eq!(out, ("{\"ok\":true}".to_string(), 0));
}

#[test]
fn wait_times_out_to_passthrough() {
    let (_d, p) = sock();
    let listener = UnixListener::bind(&p).unwrap();
    let _keep = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let _m: HookMessage = read_frame(&mut s).unwrap();
        std::thread::sleep(Duration::from_secs(5));
        drop(s);
    });
    let started = Instant::now();
    let out = run_hook(&p, &["--agent", "claude", "--event", "PermissionRequest", "--wait", "--timeout-secs", "1"], "{}");
    assert_eq!(out, (String::new(), 0));
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[test]
fn mismatched_reply_id_is_ignored() {
    let (_d, p) = sock();
    let listener = UnixListener::bind(&p).unwrap();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let _m: HookMessage = read_frame(&mut s).unwrap();
        write_frame(&mut s, &HookReply { v: 1, id: "other".into(), stdout: "nope".into(), exit_code: 2 }).unwrap();
    });
    let out = run_hook(&p, &["--agent", "claude", "--event", "PermissionRequest", "--wait"], "{}");
    assert_eq!(out, (String::new(), 0));
}

#[test]
fn server_closing_without_reply_is_passthrough() {
    let (_d, p) = sock();
    let listener = UnixListener::bind(&p).unwrap();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let _m: HookMessage = read_frame(&mut s).unwrap();
    });
    let out = run_hook(&p, &["--agent", "claude", "--event", "PermissionRequest", "--wait"], "not json");
    assert_eq!(out, (String::new(), 0));
}
```

- [ ] **Step 3: Run** `cargo test -p peekaboo-hook` → FAIL (no main).

- [ ] **Step 4: Implement** `crates/hook/src/main.rs`:
```rust
//! `peekaboo-hook`: forwards an agent hook payload to Peekaboo and relays the reply.
//! Must never block or break the agent: every failure exits 0 with no output.
use peekaboo_protocol as proto;
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 600;

struct Args {
    agent: String,
    event: String,
    wait: bool,
    timeout_secs: u64,
}

fn parse_args(mut it: impl Iterator<Item = String>) -> Args {
    let mut a = Args { agent: "unknown".into(), event: "unknown".into(), wait: false, timeout_secs: DEFAULT_TIMEOUT_SECS };
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--agent" => a.agent = it.next().unwrap_or_default(),
            "--event" => a.event = it.next().unwrap_or_default(),
            "--wait" => a.wait = true,
            "--timeout-secs" => {
                a.timeout_secs = it.next().and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_TIMEOUT_SECS)
            }
            _ => {}
        }
    }
    a
}

trait Duplex: Read + Write + Send {}
impl<T: Read + Write + Send> Duplex for T {}

#[cfg(unix)]
fn connect() -> std::io::Result<Box<dyn Duplex>> {
    Ok(Box::new(std::os::unix::net::UnixStream::connect(proto::socket_path())?))
}

#[cfg(windows)]
fn connect() -> std::io::Result<Box<dyn Duplex>> {
    let f = std::fs::OpenOptions::new().read(true).write(true).open(proto::pipe_name())?;
    Ok(Box::new(f))
}

#[cfg(unix)]
fn parent_pid() -> Option<u32> {
    Some(std::os::unix::process::parent_id())
}

#[cfg(not(unix))]
fn parent_pid() -> Option<u32> {
    None
}

fn run(args: &Args, raw: &str) -> (String, i32) {
    let payload = serde_json::from_str(raw).unwrap_or_else(|_| serde_json::Value::String(raw.to_string()));
    let msg = proto::HookMessage {
        v: proto::PROTOCOL_VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        agent: args.agent.clone(),
        event: args.event.clone(),
        cwd: std::env::current_dir().ok().map(|p| p.display().to_string()),
        pid: parent_pid(),
        payload,
        expects_reply: args.wait,
    };
    let Ok(mut stream) = connect() else { return (String::new(), 0) };
    if proto::write_frame(&mut stream, &msg).is_err() || !args.wait {
        return (String::new(), 0);
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let reply: Result<proto::HookReply, _> = proto::read_frame(&mut stream);
        let _ = tx.send(reply);
    });
    match rx.recv_timeout(Duration::from_secs(args.timeout_secs)) {
        Ok(Ok(reply)) if reply.id == msg.id => (reply.stdout, reply.exit_code),
        _ => (String::new(), 0),
    }
}

fn main() {
    let args = parse_args(std::env::args().skip(1));
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    let (out, code) = run(&args, &raw);
    if !out.is_empty() {
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(out.as_bytes());
        let _ = stdout.flush();
    }
    std::process::exit(code);
}
```

- [ ] **Step 5: Run** `cargo test -p peekaboo-hook` → PASS (6 tests).

- [ ] **Step 6: Commit** `git commit -am "feat(hook): agent-agnostic peekaboo-hook binary"` (add new files first).

---

### Task 3: Core model + Claude Code adapter

**Files:**
- Create: `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/model.rs`, `crates/core/src/adapters/mod.rs`, `crates/core/src/adapters/claude.rs`

**Interfaces:**
- Consumes: `HookMessage`.
- Produces (in `peekaboo_core::model`): `CoreEvent`, `Decision {Allow{always}, Deny{reason}, Passthrough, Answer{answers}}`, `Answer {question, answer}`, `Question`, `QuestionOption`, `SessionState`, `Session`, `PendingKind`, `PendingView`, `AlwaysRule {agent, tool}`, `HookStatus`, `AppState`, `now_secs()`.
- Produces (in `peekaboo_core::adapters`): `trait AgentAdapter { id, session_key, normalize, render_reply }`, `adapter_for(&str) -> Option<&'static dyn AgentAdapter>`.

- [ ] **Step 1: Cargo.toml**
```toml
[package]
name = "peekaboo-core"
version.workspace = true
edition.workspace = true

[dependencies]
peekaboo-protocol = { path = "../protocol" }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
dirs.workspace = true
tokio.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`crates/core/src/lib.rs`:
```rust
pub mod adapters;
pub mod model;
```
(Later tasks append `pub mod sessions; pub mod settings; pub mod core; pub mod ipc; pub mod installer;`.)

- [ ] **Step 2: `model.rs`**
```rust
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
    Allow { #[serde(default)] always: bool },
    Deny { #[serde(default)] reason: Option<String> },
    Passthrough,
    Answer { answers: Vec<Answer> },
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlwaysRule {
    pub agent: String,
    pub tool: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookStatus {
    pub agent: String,
    pub installed: bool,
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
```

- [ ] **Step 3: Write failing adapter tests** in `adapters/claude.rs` (tests module):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use peekaboo_protocol::HookMessage;
    use serde_json::json;

    fn msg(event: &str, payload: serde_json::Value) -> HookMessage {
        HookMessage { v: 1, id: "id1".into(), agent: "claude".into(), event: event.into(),
            cwd: Some("/work/shop".into()), pid: Some(9), payload, expects_reply: true }
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
        let m = msg("PreToolUse", json!({"tool_name":"AskUserQuestion","tool_input":{"questions":[
            {"question":"Which DB?","header":"DB","multiSelect":false,"options":[{"label":"Postgres","description":"SQL"},{"label":"Mongo"}]}
        ]}}));
        let ev = ClaudeAdapter.normalize(&m);
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
    }

    #[test]
    fn renders_answers_as_deny_with_reason() {
        let m = msg("PreToolUse", json!({"tool_name":"AskUserQuestion"}));
        let d = Decision::Answer { answers: vec![Answer { question: "Which DB?".into(), answer: "Postgres".into() }] };
        let (out, _) = ClaudeAdapter.render_reply(&m, &d);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(v["hookSpecificOutput"]["permissionDecisionReason"].as_str().unwrap().contains("Which DB? → Postgres"));
    }

    #[test]
    fn passthrough_renders_nothing() {
        assert_eq!(ClaudeAdapter.render_reply(&msg("PermissionRequest", json!({})), &Decision::Passthrough), (String::new(), 0));
    }
}
```

- [ ] **Step 4: Run** `cargo test -p peekaboo-core` → FAIL.

- [ ] **Step 5: Implement** `adapters/mod.rs`:
```rust
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
```

`adapters/claude.rs` (above tests):
```rust
//! Claude Code hooks: https://docs.claude.com/en/docs/claude-code/hooks
use super::{str_field, AgentAdapter};
use crate::model::{Answer, CoreEvent, Decision, Question};
use peekaboo_protocol::HookMessage;
use serde_json::{json, Value};

pub struct ClaudeAdapter;

fn parse_questions(input: &Value) -> Vec<Question> {
    input.get("questions").cloned().and_then(|q| serde_json::from_value(q).ok()).unwrap_or_default()
}

fn format_answers(answers: &[Answer]) -> String {
    let mut s = String::from("The user answered these questions in Peekaboo. Use these answers and do not ask again:\n");
    for a in answers {
        s.push_str(&format!("- {} → {}\n", a.question, a.answer));
    }
    s
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
        let input = p.get("tool_input").cloned().unwrap_or(Value::Null);
        match msg.event.as_str() {
            "SessionStart" => vec![CoreEvent::SessionStarted],
            "SessionEnd" => vec![CoreEvent::SessionEnded],
            "UserPromptSubmit" => vec![CoreEvent::PromptSubmitted { prompt: str_field(p, "prompt") }],
            "PreToolUse" if tool == "AskUserQuestion" => vec![CoreEvent::QuestionAsked { questions: parse_questions(&input) }],
            "PreToolUse" => vec![CoreEvent::ToolStarted { tool, input }],
            "PostToolUse" => vec![CoreEvent::ToolFinished { tool }],
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
                "decision": {"behavior": "deny", "message": reason.clone().unwrap_or_else(|| "Denied from Peekaboo".into())}}}),
            ("PreToolUse", Decision::Answer { answers }) => json!({"hookSpecificOutput": {
                "hookEventName": "PreToolUse", "permissionDecision": "deny",
                "permissionDecisionReason": format_answers(answers)}}),
            ("PreToolUse", Decision::Deny { reason }) => json!({"hookSpecificOutput": {
                "hookEventName": "PreToolUse", "permissionDecision": "deny",
                "permissionDecisionReason": reason.clone().unwrap_or_else(|| "Denied from Peekaboo".into())}}),
            _ => return (String::new(), 0),
        };
        (out.to_string(), 0)
    }
}
```
Before finalizing, confirm the `PermissionRequest` output shape and `AskUserQuestion` input shape against the current Claude Code hooks docs (WebFetch `https://docs.claude.com/en/docs/claude-code/hooks`); adjust code and tests together if they differ.

- [ ] **Step 6: Run** `cargo test -p peekaboo-core` → PASS.

- [ ] **Step 7: Commit** `git add -A && git commit -m "feat(core): event model and Claude Code adapter"`

---

### Task 4: Session store + settings

**Files:**
- Create: `crates/core/src/sessions.rs`, `crates/core/src/settings.rs`; Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produces: `SessionStore::{default, apply(&mut, key:&str, agent:&str, cwd:Option<&str>, ev:&CoreEvent, now:u64), set_state(&mut, key:&str, state:SessionState, now:u64), list(&self, now:u64) -> Vec<Session>}`; `STALE_AFTER_SECS = 1800`.
- Produces: `Settings { approval_timeout_secs: u64, rules: Vec<AlwaysRule> }` with `load(&Path) -> Settings`, `save(&self, &Path) -> io::Result<()>`, `allows(&self, agent, tool) -> bool`, `add_rule(AlwaysRule)`, `remove_rule(&AlwaysRule)`.

- [ ] **Step 1: Failing tests** — `sessions.rs` tests:
```rust
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
```
`settings.rs` tests:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let d = tempfile::tempdir().unwrap();
        let s = Settings::load(&d.path().join("settings.json"));
        assert_eq!(s.approval_timeout_secs, 600);
        assert!(s.rules.is_empty());
    }

    #[test]
    fn save_load_round_trip_and_rules() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("nested/settings.json");
        let mut s = Settings::default();
        let r = AlwaysRule { agent: "claude".into(), tool: "Read".into() };
        s.add_rule(r.clone());
        s.add_rule(r.clone());
        assert_eq!(s.rules.len(), 1);
        assert!(s.allows("claude", "Read"));
        assert!(!s.allows("claude", "Bash"));
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p), s);
        s.remove_rule(&r);
        assert!(!s.allows("claude", "Read"));
    }

    #[test]
    fn corrupt_file_is_backed_up_and_defaults_used() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(Settings::load(&p), Settings::default());
        assert!(d.path().join("settings.json.bad").exists());
    }
}
```

- [ ] **Step 2: Run** `cargo test -p peekaboo-core` → FAIL.

- [ ] **Step 3: Implement** `sessions.rs`:
```rust
use crate::model::{project_name, CoreEvent, Session, SessionState};
use serde_json::Value;
use std::collections::HashMap;

pub const STALE_AFTER_SECS: u64 = 30 * 60;

#[derive(Default)]
pub struct SessionStore {
    sessions: HashMap<String, Session>,
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max).collect::<String>()) }
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
            CoreEvent::PermissionRequested { tool, .. } => (SessionState::WaitingApproval, Some(format!("Needs approval: {tool}"))),
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
```
`settings.rs`:
```rust
use crate::model::AlwaysRule;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub approval_timeout_secs: u64,
    pub rules: Vec<AlwaysRule>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { approval_timeout_secs: 600, rules: Vec::new() }
    }
}

impl Settings {
    /// Missing file → defaults. Corrupt file → renamed to `<name>.bad`, defaults returned.
    pub fn load(path: &Path) -> Settings {
        let Ok(text) = std::fs::read_to_string(path) else { return Settings::default() };
        match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("peekaboo: corrupt settings {}: {e}", path.display());
                let _ = std::fs::rename(path, path.with_extension("json.bad"));
                Settings::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }

    pub fn allows(&self, agent: &str, tool: &str) -> bool {
        self.rules.iter().any(|r| r.agent == agent && r.tool == tool)
    }

    pub fn add_rule(&mut self, rule: AlwaysRule) {
        if !self.rules.contains(&rule) {
            self.rules.push(rule);
        }
    }

    pub fn remove_rule(&mut self, rule: &AlwaysRule) {
        self.rules.retain(|r| r != rule);
    }
}
```
Add `pub mod sessions; pub mod settings;` to `lib.rs`.

- [ ] **Step 4: Run** `cargo test -p peekaboo-core` → PASS.
- [ ] **Step 5: Commit** `git add -A && git commit -m "feat(core): session store and settings"`

---

### Task 5: Core orchestration

**Files:**
- Create: `crates/core/src/core.rs`; Modify: `crates/core/src/lib.rs` (`pub mod core; pub use crate::core::{Core, Emitter, Handled};`)

**Interfaces:**
- Consumes: adapters, SessionStore, Settings.
- Produces:
  - `type Emitter = Arc<dyn Fn() + Send + Sync>`
  - `enum Handled { Reply(HookReply), Wait { msg: HookMessage, rx: tokio::sync::oneshot::Receiver<Decision> } }`
  - `Core::new(settings_path: PathBuf, emit: Emitter) -> Core`
  - `Core::handle(&self, msg: HookMessage) -> Handled`
  - `Core::decide(&self, id: &str, d: Decision) -> Result<(), String>`
  - `Core::cancel(&self, id: &str)`
  - `Core::render_reply(&self, msg: &HookMessage, d: &Decision) -> HookReply`
  - `Core::snapshot(&self) -> AppState` (hooks left empty; filled by the app)
  - `Core::remove_rule(&self, rule: &AlwaysRule) -> Result<(), String>`
  - `Core::approval_timeout_secs(&self) -> u64`

- [ ] **Step 1: Failing tests** in `core.rs`:
```rust
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

    #[test]
    fn always_creates_rule_and_auto_allows_next_time() {
        let (d, c, _) = core();
        let Handled::Wait { .. } = c.handle(perm("1", "s", "Read")) else { panic!() };
        c.decide("1", Decision::Allow { always: true }).unwrap();
        assert_eq!(c.snapshot().rules, vec![AlwaysRule { agent: "claude".into(), tool: "Read".into() }]);
        match c.handle(perm("2", "s", "Read")) {
            Handled::Reply(r) => assert!(r.stdout.contains("allow")),
            _ => panic!("expected auto-allow"),
        }
        assert!(Settings::load(&d.path().join("settings.json")).allows("claude", "Read"));
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
```

- [ ] **Step 2: Run** → FAIL.

- [ ] **Step 3: Implement** `core.rs`:
```rust
use crate::adapters::adapter_for;
use crate::model::*;
use crate::sessions::SessionStore;
use crate::settings::Settings;
use peekaboo_protocol::{HookMessage, HookReply, PROTOCOL_VERSION};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

pub type Emitter = Arc<dyn Fn() + Send + Sync>;

pub enum Handled {
    /// Answer the hook now.
    Reply(HookReply),
    /// Hold the connection until the user decides (or the hook goes away).
    Wait { msg: HookMessage, rx: oneshot::Receiver<Decision> },
}

struct Pending {
    view: PendingView,
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
        Core { inner: Mutex::new(Inner { sessions: SessionStore::default(), pending: HashMap::new(), settings }), emit, settings_path }
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
                        if inner.settings.allows(&msg.agent, &tool) {
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
                inner.pending.insert(msg.id.clone(), Pending { view, tx });
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
            if let (Decision::Allow { always: true }, PendingKind::Permission { tool, .. }) = (&decision, &p.view.kind) {
                inner.settings.add_rule(AlwaysRule { agent: p.view.agent.clone(), tool: tool.clone() });
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
        pending.sort_by_key(|p| p.created_at);
        AppState { sessions: inner.sessions.list(now_secs()), pending, rules: inner.settings.rules.clone(), hooks: vec![] }
    }
}
```

- [ ] **Step 4: Run** `cargo test -p peekaboo-core` → PASS.
- [ ] **Step 5: Commit** `git add -A && git commit -m "feat(core): approval orchestration and Always rules"`

---

### Task 6: IPC server + end-to-end tests

**Files:**
- Create: `crates/core/src/ipc.rs`, `crates/hook/tests/e2e.rs`; Modify: `crates/core/src/lib.rs` (`pub mod ipc;`), `crates/hook/Cargo.toml` (dev-deps)

**Interfaces:**
- Produces: `ipc::serve(core: Arc<Core>) -> io::Result<()>` (platform default address); `#[cfg(unix)] ipc::serve_unix(core: Arc<Core>, path: &Path) -> io::Result<()>`; `#[cfg(windows)] ipc::serve_pipe(core, name: &str)`.

- [ ] **Step 1: Failing tests** in `ipc.rs`:
```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use peekaboo_protocol::{encode_frame, HookReply};
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    async fn start() -> (tempfile::TempDir, std::path::PathBuf, Arc<Core>) {
        let d = tempfile::tempdir().unwrap();
        let sock = d.path().join("pb.sock");
        let core = Arc::new(Core::new(d.path().join("settings.json"), Arc::new(|| {})));
        let (c2, s2) = (core.clone(), sock.clone());
        tokio::spawn(async move { serve_unix(c2, &s2).await.unwrap() });
        for _ in 0..50 {
            if UnixStream::connect(&sock).await.is_ok() { break }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        (d, sock, core)
    }

    fn stop_msg(id: &str) -> HookMessage {
        HookMessage { v: 1, id: id.into(), agent: "claude".into(), event: "Stop".into(), cwd: None, pid: None,
            payload: json!({"session_id": "s"}), expects_reply: true }
    }

    async fn round_trip(sock: &Path, m: &HookMessage) -> HookReply {
        let mut s = UnixStream::connect(sock).await.unwrap();
        s.write_all(&encode_frame(m).unwrap()).await.unwrap();
        read_frame_async(&mut s).await.unwrap()
    }

    #[tokio::test]
    async fn socket_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, sock, _) = start().await;
        assert_eq!(std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[tokio::test]
    async fn replies_passthrough_for_notification_needing_reply() {
        let (_d, sock, _) = start().await;
        let r = round_trip(&sock, &stop_msg("x")).await;
        assert_eq!((r.id.as_str(), r.stdout.as_str()), ("x", ""));
    }

    #[tokio::test]
    async fn garbage_frame_does_not_kill_server() {
        let (_d, sock, _) = start().await;
        let mut s = UnixStream::connect(&sock).await.unwrap();
        s.write_all(&[0, 0, 0, 5, b'n', b'o', b'p', b'e', b'!']).await.unwrap();
        let mut buf = [0u8; 1];
        assert_eq!(s.read(&mut buf).await.unwrap(), 0); // server closed this connection
        let r = round_trip(&sock, &stop_msg("y")).await;
        assert_eq!(r.id, "y");
    }

    #[tokio::test]
    async fn serve_replaces_stale_socket_file() {
        let d = tempfile::tempdir().unwrap();
        let sock = d.path().join("pb.sock");
        std::fs::write(&sock, b"stale").unwrap();
        let core = Arc::new(Core::new(d.path().join("settings.json"), Arc::new(|| {})));
        let s2 = sock.clone();
        tokio::spawn(async move { serve_unix(core, &s2).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(round_trip(&sock, &stop_msg("z")).await.id, "z");
    }
}
```

- [ ] **Step 2: Run** → FAIL.

- [ ] **Step 3: Implement** `ipc.rs`:
```rust
//! Accepts `peekaboo-hook` connections and routes them through `Core`.
use crate::core::{Core, Handled};
use peekaboo_protocol::{decode_len, encode_frame, FrameError, HookMessage};
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) async fn read_frame_async<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R) -> Result<T, FrameError> {
    let mut header = [0u8; 4];
    r.read_exact(&mut header).await?;
    let mut body = vec![0u8; decode_len(header)?];
    r.read_exact(&mut body).await?;
    Ok(serde_json::from_slice(&body)?)
}

async fn write_frame_async<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, msg: &T) -> Result<(), FrameError> {
    w.write_all(&encode_frame(msg)?).await?;
    w.flush().await?;
    Ok(())
}

async fn handle_conn<S: AsyncRead + AsyncWrite + Unpin>(core: Arc<Core>, mut stream: S) -> Result<(), FrameError> {
    let msg: HookMessage = match read_frame_async(&mut stream).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("peekaboo: dropped bad hook frame: {e}");
            return Ok(());
        }
    };
    let expects_reply = msg.expects_reply;
    match core.handle(msg) {
        Handled::Reply(reply) => {
            if expects_reply {
                write_frame_async(&mut stream, &reply).await?;
            }
        }
        Handled::Wait { msg, rx } => {
            let mut probe = [0u8; 1];
            // A read completing means the hook hung up (it never sends a second frame).
            let decision = tokio::select! {
                d = rx => d.ok(),
                _ = stream.read(&mut probe) => None,
            };
            match decision {
                Some(d) => write_frame_async(&mut stream, &core.render_reply(&msg, &d)).await?,
                None => core.cancel(&msg.id),
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
pub async fn serve_unix(core: Arc<Core>, path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(path);
    let listener = tokio::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    loop {
        let (stream, _) = listener.accept().await?;
        let core = core.clone();
        tokio::spawn(async move {
            let _ = handle_conn(core, stream).await;
        });
    }
}

#[cfg(windows)]
pub async fn serve_pipe(core: Arc<Core>, name: &str) -> std::io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;
    let mut server = ServerOptions::new().first_pipe_instance(true).create(name)?;
    loop {
        server.connect().await?;
        let connected = server;
        server = ServerOptions::new().create(name)?;
        let core = core.clone();
        tokio::spawn(async move {
            let _ = handle_conn(core, connected).await;
        });
    }
}

pub async fn serve(core: Arc<Core>) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        serve_unix(core, &peekaboo_protocol::socket_path()).await
    }
    #[cfg(windows)]
    {
        serve_pipe(core, &peekaboo_protocol::pipe_name()).await
    }
}
```
Run `cargo test -p peekaboo-core` → PASS.

- [ ] **Step 4: E2E tests with the real hook.** Add to `crates/hook/Cargo.toml`:
```toml
[dev-dependencies]
tempfile.workspace = true
peekaboo-core = { path = "../core" }
tokio.workspace = true
```
`crates/hook/tests/e2e.rs`:
```rust
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
        if core.snapshot().pending.len() == n { return }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("pending never reached {n}");
}

const PERM: &str = r#"{"session_id":"s1","tool_name":"Bash","tool_input":{"command":"rm -rf build"}}"#;

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
async fn killed_hook_clears_pending() {
    let (_d, sock, core) = setup().await;
    let mut child = spawn_hook(&sock, "PermissionRequest", PERM);
    wait_pending(&core, 1).await;
    child.kill().unwrap();
    let _ = child.wait();
    wait_pending(&core, 0).await;
    assert_ne!(core.snapshot().sessions[0].state, peekaboo_core::model::SessionState::WaitingApproval);
}
```
Run `cargo test` (workspace default members) → all PASS.

- [ ] **Step 5: Commit** `git add -A && git commit -m "feat(core): IPC server with disconnect handling + e2e tests"`

---

### Task 7: Claude Code hook installer

**Files:**
- Create: `crates/core/src/installer/mod.rs`, `crates/core/src/installer/claude.rs`; Modify: `crates/core/src/lib.rs` (`pub mod installer;`)

**Interfaces:**
- Produces: `installer::{InstallCtx { hook_bin: PathBuf, backup_dir: PathBuf, timeout_secs: u64 }, InstallError, HookInstaller, installers(&InstallCtx) -> Vec<Box<dyn HookInstaller>>, HOOK_MARKER}`; `trait HookInstaller { agent(&self) -> &'static str; status(&self) -> HookStatus; install(&self) -> Result<Option<PathBuf>, InstallError>; uninstall(&self) -> Result<(), InstallError>; }`; `claude::ClaudeInstaller { config_path: PathBuf, ctx: InstallCtx }`.

- [ ] **Step 1: Failing tests** in `installer/claude.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn inst(dir: &std::path::Path) -> ClaudeInstaller {
        ClaudeInstaller {
            config_path: dir.join(".claude/settings.json"),
            ctx: InstallCtx { hook_bin: "/Apps/Peekaboo/peekaboo-hook".into(), backup_dir: dir.join("backups"), timeout_secs: 600 },
        }
    }

    fn read(p: &std::path::Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn fresh_install_creates_config_with_all_events() {
        let d = tempfile::tempdir().unwrap();
        let i = inst(d.path());
        assert!(!i.status().installed);
        assert_eq!(i.install().unwrap(), None); // nothing to back up
        let v = read(&i.config_path);
        let perm = &v["hooks"]["PermissionRequest"][0]["hooks"][0];
        assert!(perm["command"].as_str().unwrap().contains("--event PermissionRequest --wait --timeout-secs 600"));
        assert_eq!(perm["timeout"], 610);
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "AskUserQuestion");
        for e in ["SessionStart", "SessionEnd", "UserPromptSubmit", "PostToolUse", "Notification", "Stop"] {
            assert!(v["hooks"][e].is_array(), "{e}");
        }
        assert!(i.status().installed);
    }

    #[test]
    fn preserves_user_hooks() {
        let d = tempfile::tempdir().unwrap();
        let i = inst(d.path());
        std::fs::create_dir_all(i.config_path.parent().unwrap()).unwrap();
        let original = json!({"model": "opus", "hooks": {"PreToolUse": [
            {"matcher": "Bash", "hooks": [{"type": "command", "command": "my-linter"}]}]}});
        std::fs::write(&i.config_path, original.to_string()).unwrap();
        let backup = i.install().unwrap().expect("backup written");
        assert_eq!(read(&backup), original);
        let v = read(&i.config_path);
        assert_eq!(v["model"], "opus");
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "my-linter");
        i.uninstall().unwrap();
        assert_eq!(read(&i.config_path), original);
    }

    #[test]
    fn install_is_idempotent() {
        let d = tempfile::tempdir().unwrap();
        let i = inst(d.path());
        i.install().unwrap();
        let once = read(&i.config_path);
        i.install().unwrap();
        assert_eq!(read(&i.config_path), once);
    }

    #[test]
    fn refuses_malformed_config() {
        let d = tempfile::tempdir().unwrap();
        let i = inst(d.path());
        std::fs::create_dir_all(i.config_path.parent().unwrap()).unwrap();
        std::fs::write(&i.config_path, "{oops").unwrap();
        assert!(matches!(i.install(), Err(InstallError::Malformed(_))));
        assert_eq!(std::fs::read_to_string(&i.config_path).unwrap(), "{oops");
        assert!(i.status().error.is_some());
    }
}
```

- [ ] **Step 2: Run** → FAIL.

- [ ] **Step 3: Implement** `installer/mod.rs`:
```rust
use crate::model::HookStatus;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub mod claude;

/// Every command we install contains this; uninstall removes exactly those entries.
pub const HOOK_MARKER: &str = "peekaboo-hook";

#[derive(Debug, Clone)]
pub struct InstallCtx {
    pub hook_bin: PathBuf,
    pub backup_dir: PathBuf,
    pub timeout_secs: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("config is not a valid JSON object: {0}")]
    Malformed(String),
}

pub trait HookInstaller: Send + Sync {
    fn agent(&self) -> &'static str;
    fn status(&self) -> HookStatus;
    /// Returns the backup path if an existing config was backed up.
    fn install(&self) -> Result<Option<PathBuf>, InstallError>;
    fn uninstall(&self) -> Result<(), InstallError>;
}

pub fn installers(ctx: &InstallCtx) -> Vec<Box<dyn HookInstaller>> {
    vec![Box::new(claude::ClaudeInstaller::new(ctx.clone()))]
}

pub(crate) fn hook_command(bin: &Path, agent: &str, event: &str, wait: bool, timeout_secs: u64) -> String {
    let mut cmd = format!("\"{}\" --agent {agent} --event {event}", bin.display());
    if wait {
        cmd.push_str(&format!(" --wait --timeout-secs {timeout_secs}"));
    }
    cmd
}

/// Reads a JSON-object config. Missing file → empty object.
pub(crate) fn read_config(path: &Path) -> Result<Map<String, Value>, InstallError> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(m)) => Ok(m),
            Ok(_) => Err(InstallError::Malformed(path.display().to_string())),
            Err(e) => Err(InstallError::Malformed(format!("{}: {e}", path.display()))),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn write_config(path: &Path, cfg: &Map<String, Value>) -> Result<(), InstallError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(cfg).map_err(|e| InstallError::Malformed(e.to_string()))?)?;
    Ok(())
}

pub(crate) fn backup(path: &Path, backup_dir: &Path, agent: &str) -> Result<Option<PathBuf>, InstallError> {
    if !path.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(backup_dir)?;
    let dest = backup_dir.join(format!("{agent}-settings-{}.json", crate::model::now_secs()));
    std::fs::copy(path, &dest)?;
    Ok(Some(dest))
}
```
`installer/claude.rs` (above tests):
```rust
use super::*;
use serde_json::json;

/// (event, matcher, waits for a reply)
const EVENTS: &[(&str, Option<&str>, bool)] = &[
    ("SessionStart", None, false),
    ("SessionEnd", None, false),
    ("UserPromptSubmit", None, false),
    ("PostToolUse", Some("*"), false),
    ("Notification", None, false),
    ("Stop", None, false),
    ("PreToolUse", Some("AskUserQuestion"), true),
    ("PermissionRequest", Some("*"), true),
];
const NOTIFY_TIMEOUT_SECS: u64 = 5;

pub struct ClaudeInstaller {
    pub config_path: PathBuf,
    pub ctx: InstallCtx,
}

impl ClaudeInstaller {
    pub fn new(ctx: InstallCtx) -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self { config_path: home.join(".claude").join("settings.json"), ctx }
    }
}

fn is_ours(hook: &Value) -> bool {
    hook.get("command").and_then(Value::as_str).is_some_and(|c| c.contains(HOOK_MARKER))
}

/// Removes every hook entry we own, dropping groups/events/the hooks key if they become empty.
fn strip_ours(cfg: &mut Map<String, Value>) {
    let Some(Value::Object(hooks)) = cfg.get_mut("hooks") else { return };
    for groups in hooks.values_mut() {
        if let Value::Array(groups) = groups {
            for g in groups.iter_mut() {
                if let Some(Value::Array(hs)) = g.get_mut("hooks") {
                    hs.retain(|h| !is_ours(h));
                }
            }
            groups.retain(|g| g.get("hooks").and_then(Value::as_array).is_none_or(|h| !h.is_empty()));
        }
    }
    hooks.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
    if hooks.is_empty() {
        cfg.remove("hooks");
    }
}

impl HookInstaller for ClaudeInstaller {
    fn agent(&self) -> &'static str {
        "claude"
    }

    fn status(&self) -> HookStatus {
        let (installed, error) = match read_config(&self.config_path) {
            Ok(cfg) => (Value::Object(cfg).to_string().contains(HOOK_MARKER), None),
            Err(e) => (false, Some(e.to_string())),
        };
        HookStatus { agent: "claude".into(), installed, config_path: self.config_path.display().to_string(), error }
    }

    fn install(&self) -> Result<Option<PathBuf>, InstallError> {
        let mut cfg = read_config(&self.config_path)?;
        let backup = backup(&self.config_path, &self.ctx.backup_dir, "claude")?;
        strip_ours(&mut cfg);
        let hooks = cfg.entry("hooks").or_insert_with(|| json!({}));
        let Value::Object(hooks) = hooks else { return Err(InstallError::Malformed("\"hooks\" is not an object".into())) };
        for (event, matcher, wait) in EVENTS {
            let timeout = if *wait { self.ctx.timeout_secs + 10 } else { NOTIFY_TIMEOUT_SECS };
            let mut group = json!({"hooks": [{
                "type": "command",
                "command": hook_command(&self.ctx.hook_bin, "claude", event, *wait, self.ctx.timeout_secs),
                "timeout": timeout,
            }]});
            if let Some(m) = matcher {
                group["matcher"] = json!(m);
            }
            let list = hooks.entry(event.to_string()).or_insert_with(|| json!([]));
            match list {
                Value::Array(a) => a.push(group),
                _ => return Err(InstallError::Malformed(format!("hooks.{event} is not an array"))),
            }
        }
        write_config(&self.config_path, &cfg)?;
        Ok(backup)
    }

    fn uninstall(&self) -> Result<(), InstallError> {
        let mut cfg = read_config(&self.config_path)?;
        backup(&self.config_path, &self.ctx.backup_dir, "claude")?;
        strip_ours(&mut cfg);
        write_config(&self.config_path, &cfg)
    }
}
```
(`Option::is_none_or` needs Rust ≥ 1.82.)

- [ ] **Step 4: Run** `cargo test -p peekaboo-core` → PASS.
- [ ] **Step 5: Commit** `git add -A && git commit -m "feat(core): Claude Code hook installer with backups"`

---

### Task 8: Tauri shell, overlay window, commands

**Files:**
- Create: `package.json`, `vite.config.ts`, `tsconfig.json`, `index.html`, `src/main.tsx`, `src/App.tsx` (temporary), `src/styles.css`, `src-tauri/{Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json, icons/*, src/main.rs, src/lib.rs, src/commands.rs, src/overlay.rs}`, `assets/boo.svg`
- Modify: `Cargo.toml` (add `src-tauri` to members)

**Interfaces:**
- Consumes: `Core`, `ipc::serve`, `installer::{InstallCtx, installers}`, model types.
- Produces Tauri commands (JS names): `get_state() -> AppState`, `decide({id, decision})`, `install_hooks({agent})`, `uninstall_hooks({agent})`, `remove_rule({agent, tool})`, `set_expanded({expanded})`; events `state:changed`, `overlay:toggle`.

- [ ] **Step 1: Frontend scaffold**
```bash
npm init -y
npm i react react-dom @tauri-apps/api
npm i -D @tauri-apps/cli typescript vite @vitejs/plugin-react @types/react @types/react-dom vitest jsdom @testing-library/react @testing-library/user-event @testing-library/jest-dom
```
`package.json` scripts: `{"dev":"vite","build":"tsc --noEmit && vite build","test":"vitest run","tauri":"tauri"}` and `"type":"module"`, `"name":"peekaboo"`, `"private":true`.

`vite.config.ts`:
```ts
/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  test: { environment: "jsdom", setupFiles: ["./src/test-setup.ts"] },
});
```
`src/test-setup.ts`: `import "@testing-library/jest-dom/vitest";`

`tsconfig.json`:
```json
{ "compilerOptions": { "target": "ES2022", "lib": ["ES2022", "DOM"], "module": "ESNext", "moduleResolution": "bundler",
  "jsx": "react-jsx", "strict": true, "skipLibCheck": true, "types": ["vitest/globals"] }, "include": ["src"] }
```
`index.html`:
```html
<!doctype html>
<html lang="en">
  <head><meta charset="UTF-8" /><title>Peekaboo</title></head>
  <body><div id="root"></div><script type="module" src="/src/main.tsx"></script></body>
</html>
```
`src/main.tsx`:
```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
```
`src/App.tsx` (temporary, replaced in Task 9): `export default function App() { return <div className="pill">👻 Peekaboo</div>; }`
`src/styles.css`: `html, body { margin: 0; background: transparent; overflow: hidden; font-family: -apple-system, system-ui, sans-serif; } .pill { background:#111; color:#fff; border-radius:0 0 18px 18px; height:44px; display:flex; align-items:center; justify-content:center; }`

- [ ] **Step 2: Icons.** `assets/boo.svg`:
```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <rect width="1024" height="1024" rx="224" fill="#16131f"/>
  <path d="M512 190c-150 0-260 116-260 270v330l65-55 65 55 65-55 65 55 65-55 65 55 65-55 65 55V460c0-154-110-270-260-270z" fill="#f4f1ff"/>
  <ellipse cx="430" cy="470" rx="38" ry="52" fill="#16131f"/>
  <ellipse cx="594" cy="470" rx="38" ry="52" fill="#16131f"/>
</svg>
```
Run `npx tauri icon assets/boo.svg -o src-tauri/icons`. If SVG input is rejected: `qlmanage -t -s 1024 -o /tmp assets/boo.svg && npx tauri icon /tmp/boo.svg.png -o src-tauri/icons`.

- [ ] **Step 3: Rust shell.** `src-tauri/Cargo.toml`:
```toml
[package]
name = "peekaboo"
version.workspace = true
edition.workspace = true

[lib]
name = "peekaboo_app"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = ["macos-private-api"] }
tauri-plugin-global-shortcut = "2"
peekaboo-core = { path = "../crates/core" }
serde_json.workspace = true

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
```
`build.rs`: `fn main() { tauri_build::build() }`
`src/main.rs`:
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    peekaboo_app::run()
}
```
`tauri.conf.json`:
```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Peekaboo",
  "version": "0.1.0",
  "identifier": "ai.devxlabs.peekaboo",
  "build": { "frontendDist": "../dist", "devUrl": "http://localhost:1420", "beforeDevCommand": "npm run dev", "beforeBuildCommand": "npm run build" },
  "app": {
    "macOSPrivateApi": true,
    "windows": [{ "label": "main", "title": "Peekaboo", "width": 220, "height": 44, "decorations": false, "transparent": true,
      "alwaysOnTop": true, "resizable": false, "skipTaskbar": true, "visible": false, "shadow": false, "focus": false }],
    "security": { "csp": null }
  },
  "bundle": { "active": true, "targets": "all",
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.icns", "icons/icon.ico"] }
}
```
`capabilities/default.json`:
```json
{ "identifier": "default", "windows": ["main"], "permissions": ["core:default"] }
```
`src/overlay.rs`:
```rust
use tauri::{LogicalPosition, LogicalSize, WebviewWindow};

const COLLAPSED: (f64, f64) = (220.0, 44.0);
const EXPANDED: (f64, f64) = (440.0, 560.0);

/// Sizes the overlay and pins it to the top-center of its monitor (over the notch on Macs).
pub fn place(w: &WebviewWindow, expanded: bool) -> tauri::Result<()> {
    let (width, height) = if expanded { EXPANDED } else { COLLAPSED };
    w.set_size(LogicalSize::new(width, height))?;
    if let Some(m) = w.current_monitor()?.or(w.primary_monitor()?) {
        let scale = m.scale_factor();
        let x = m.position().x as f64 / scale + (m.size().width as f64 / scale - width) / 2.0;
        w.set_position(LogicalPosition::new(x, m.position().y as f64 / scale))?;
    }
    Ok(())
}

pub fn setup(w: &WebviewWindow) -> tauri::Result<()> {
    place(w, false)?;
    #[cfg(target_os = "macos")]
    raise_above_menu_bar(w);
    w.set_visible_on_all_workspaces(true)?;
    w.show()
}

#[cfg(target_os = "macos")]
fn raise_above_menu_bar(w: &WebviewWindow) {
    const NS_STATUS_WINDOW_LEVEL: isize = 25;
    if let Ok(ptr) = w.ns_window() {
        unsafe {
            let win: &objc2::runtime::AnyObject = &*(ptr as *mut objc2::runtime::AnyObject);
            let _: () = objc2::msg_send![win, setLevel: NS_STATUS_WINDOW_LEVEL];
        }
    }
}

#[tauri::command]
pub fn set_expanded(window: WebviewWindow, expanded: bool) -> Result<(), String> {
    place(&window, expanded).map_err(|e| e.to_string())
}
```
`src/commands.rs`:
```rust
use peekaboo_core::installer::{installers, InstallCtx};
use peekaboo_core::model::{AlwaysRule, AppState, Decision};
use peekaboo_core::Core;
use std::sync::Arc;
use tauri::State;

pub struct AppCtx {
    pub core: Arc<Core>,
    pub install: InstallCtx,
}

#[tauri::command]
pub fn get_state(ctx: State<AppCtx>) -> AppState {
    let mut s = ctx.core.snapshot();
    s.hooks = installers(&ctx.install).iter().map(|i| i.status()).collect();
    s
}

#[tauri::command]
pub fn decide(ctx: State<AppCtx>, id: String, decision: Decision) -> Result<(), String> {
    ctx.core.decide(&id, decision)
}

fn with_installer<T>(ctx: &AppCtx, agent: &str, f: impl FnOnce(&dyn peekaboo_core::installer::HookInstaller) -> Result<T, peekaboo_core::installer::InstallError>) -> Result<T, String> {
    let all = installers(&ctx.install);
    let i = all.iter().find(|i| i.agent() == agent).ok_or_else(|| format!("unknown agent {agent}"))?;
    f(i.as_ref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn install_hooks(ctx: State<AppCtx>, agent: String) -> Result<(), String> {
    with_installer(&ctx, &agent, |i| i.install().map(|_| ()))
}

#[tauri::command]
pub fn uninstall_hooks(ctx: State<AppCtx>, agent: String) -> Result<(), String> {
    with_installer(&ctx, &agent, |i| i.uninstall())
}

#[tauri::command]
pub fn remove_rule(ctx: State<AppCtx>, agent: String, tool: String) -> Result<(), String> {
    ctx.core.remove_rule(&AlwaysRule { agent, tool })
}
```
`src/lib.rs`:
```rust
mod commands;
mod overlay;

use commands::AppCtx;
use peekaboo_core::installer::InstallCtx;
use peekaboo_core::{ipc, Core};
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::ShortcutState;

/// The hook binary ships next to the app executable (sidecar in bundles, same target dir in dev).
fn hook_bin_path() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.with_file_name(format!("peekaboo-hook{}", std::env::consts::EXE_SUFFIX))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts(["CommandOrControl+Alt+P"])
                .expect("valid shortcut")
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let _ = app.emit("overlay:toggle", ());
                    }
                })
                .build(),
        )
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let data_dir = app.path().app_data_dir()?;
            let handle = app.handle().clone();
            let core = Arc::new(Core::new(
                data_dir.join("settings.json"),
                Arc::new(move || {
                    let _ = handle.emit("state:changed", ());
                }),
            ));
            let install = InstallCtx { hook_bin: hook_bin_path(), backup_dir: data_dir.join("backups"), timeout_secs: core.approval_timeout_secs() };
            app.manage(AppCtx { core: core.clone(), install });
            tauri::async_runtime::spawn(async move {
                if let Err(e) = ipc::serve(core).await {
                    eprintln!("peekaboo: IPC server stopped: {e}");
                }
            });
            overlay::setup(&app.get_webview_window("main").expect("main window"))?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::decide,
            commands::install_hooks,
            commands::uninstall_hooks,
            commands::remove_rule,
            overlay::set_expanded,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Peekaboo");
}
```
Add `"src-tauri"` back to workspace `members`.

- [ ] **Step 4: Verify.** `cargo build -p peekaboo-hook && npm run tauri dev`. Expected: a dark pill appears at the top-center of the screen (over the notch on a notch Mac), no Dock icon. Fix compile errors against actual Tauri 2 / plugin APIs if signatures differ. `cargo test` still PASS.

- [ ] **Step 5: Commit** `git add -A && git commit -m "feat(app): Tauri shell, overlay window, commands"`

---

### Task 9: Overlay UI

**Files:**
- Create: `src/types.ts`, `src/api.ts`, `src/components/{Ghost,Pill,SessionList,ApprovalCard,QuestionCard,SettingsPanel}.tsx`, `src/components/*.test.tsx`; Replace: `src/App.tsx`, `src/styles.css`

**Interfaces:**
- Consumes: Task 8 commands/events; JSON shapes from `model.rs` (camelCase).
- Produces: components with props listed below.

- [ ] **Step 1: Types + API.** `src/types.ts`:
```ts
export type SessionState = "working" | "waitingApproval" | "waitingAnswer" | "idle" | "ended";
export interface Session { key: string; agent: string; cwd: string | null; project: string; state: SessionState; lastAction: string | null; updatedAt: number }
export interface QuestionOption { label: string; description?: string | null }
export interface Question { question: string; header?: string | null; options: QuestionOption[]; multiSelect: boolean }
interface PendingBase { id: string; agent: string; sessionKey: string; project: string; createdAt: number }
export type PendingView =
  | (PendingBase & { kind: "permission"; tool: string; input: Record<string, unknown> | null })
  | (PendingBase & { kind: "question"; questions: Question[] });
export interface AlwaysRule { agent: string; tool: string }
export interface HookStatus { agent: string; installed: boolean; configPath: string; error: string | null }
export interface AppState { sessions: Session[]; pending: PendingView[]; rules: AlwaysRule[]; hooks: HookStatus[] }
export interface Answer { question: string; answer: string }
export type Decision =
  | { type: "allow"; always: boolean }
  | { type: "deny"; reason?: string | null }
  | { type: "passthrough" }
  | { type: "answer"; answers: Answer[] };
```
`src/api.ts`:
```ts
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppState, Decision } from "./types";

export const getState = () => invoke<AppState>("get_state");
export const decide = (id: string, decision: Decision) => invoke<void>("decide", { id, decision });
export const installHooks = (agent: string) => invoke<void>("install_hooks", { agent });
export const uninstallHooks = (agent: string) => invoke<void>("uninstall_hooks", { agent });
export const removeRule = (agent: string, tool: string) => invoke<void>("remove_rule", { agent, tool });
export const setExpanded = (expanded: boolean) => invoke<void>("set_expanded", { expanded });
export const onStateChanged = (cb: () => void) => listen("state:changed", () => cb());
export const onToggle = (cb: () => void) => listen("overlay:toggle", () => cb());
```

- [ ] **Step 2: Failing component tests.** `src/components/ApprovalCard.test.tsx`:
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApprovalCard } from "./ApprovalCard";
import type { PendingView } from "../types";

const req: PendingView = { id: "1", agent: "claude", sessionKey: "k", project: "shop", createdAt: 0,
  kind: "permission", tool: "Bash", input: { command: "rm -rf build" } };

test("shows tool and command and sends decisions", async () => {
  const onDecide = vi.fn();
  render(<ApprovalCard req={req} onDecide={onDecide} />);
  expect(screen.getByText("Bash")).toBeInTheDocument();
  expect(screen.getByText("rm -rf build")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Allow" }));
  await userEvent.click(screen.getByRole("button", { name: "Always" }));
  await userEvent.click(screen.getByRole("button", { name: "Deny" }));
  await userEvent.click(screen.getByRole("button", { name: "Terminal" }));
  expect(onDecide.mock.calls.map((c) => c[0])).toEqual([
    { type: "allow", always: false }, { type: "allow", always: true }, { type: "deny" }, { type: "passthrough" },
  ]);
});
```
`src/components/QuestionCard.test.tsx`:
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QuestionCard } from "./QuestionCard";
import type { PendingView } from "../types";

const req: PendingView = { id: "q", agent: "claude", sessionKey: "k", project: "shop", createdAt: 0, kind: "question",
  questions: [
    { question: "Which DB?", options: [{ label: "Postgres" }, { label: "Mongo" }], multiSelect: false },
    { question: "Extras?", options: [{ label: "Auth" }, { label: "Cache" }], multiSelect: true },
  ] };

test("collects single and multi answers", async () => {
  const onDecide = vi.fn();
  render(<QuestionCard req={req} onDecide={onDecide} />);
  expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  await userEvent.click(screen.getByRole("button", { name: "Postgres" }));
  await userEvent.click(screen.getByRole("button", { name: "Auth" }));
  await userEvent.click(screen.getByRole("button", { name: "Cache" }));
  await userEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(onDecide).toHaveBeenCalledWith({ type: "answer", answers: [
    { question: "Which DB?", answer: "Postgres" }, { question: "Extras?", answer: "Auth, Cache" },
  ] });
});

test("free-text answer and terminal fallback", async () => {
  const onDecide = vi.fn();
  render(<QuestionCard req={{ ...req, questions: [req.questions[0]] } as PendingView} onDecide={onDecide} />);
  await userEvent.type(screen.getByPlaceholderText("Other…"), "SQLite");
  await userEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(onDecide).toHaveBeenLastCalledWith({ type: "answer", answers: [{ question: "Which DB?", answer: "SQLite" }] });
  await userEvent.click(screen.getByRole("button", { name: "Answer in terminal" }));
  expect(onDecide).toHaveBeenLastCalledWith({ type: "passthrough" });
});
```
`src/components/SessionList.test.tsx`:
```tsx
import { render, screen } from "@testing-library/react";
import { SessionList } from "./SessionList";

test("renders sessions and empty state", () => {
  const { rerender } = render(<SessionList sessions={[]} />);
  expect(screen.getByText(/No active agents/)).toBeInTheDocument();
  rerender(<SessionList sessions={[{ key: "k", agent: "claude", cwd: "/w/shop", project: "shop", state: "waitingApproval", lastAction: "Needs approval: Bash", updatedAt: 1 }]} />);
  expect(screen.getByText("shop")).toBeInTheDocument();
  expect(screen.getByText("Needs approval: Bash")).toBeInTheDocument();
  expect(screen.getByText("Waiting for you")).toBeInTheDocument();
});
```
`src/components/SettingsPanel.test.tsx`:
```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SettingsPanel } from "./SettingsPanel";

test("install, uninstall and remove rule", async () => {
  const p = { onInstall: vi.fn(), onUninstall: vi.fn(), onRemoveRule: vi.fn() };
  const { rerender } = render(<SettingsPanel hooks={[{ agent: "claude", installed: false, configPath: "~/.claude/settings.json", error: null }]} rules={[]} {...p} />);
  await userEvent.click(screen.getByRole("button", { name: "Install" }));
  expect(p.onInstall).toHaveBeenCalledWith("claude");
  rerender(<SettingsPanel hooks={[{ agent: "claude", installed: true, configPath: "x", error: null }]} rules={[{ agent: "claude", tool: "Read" }]} {...p} />);
  await userEvent.click(screen.getByRole("button", { name: "Uninstall" }));
  expect(p.onUninstall).toHaveBeenCalledWith("claude");
  await userEvent.click(screen.getByRole("button", { name: "Remove Read" }));
  expect(p.onRemoveRule).toHaveBeenCalledWith({ agent: "claude", tool: "Read" });
});
```

- [ ] **Step 3: Run** `npm test` → FAIL.

- [ ] **Step 4: Implement components.**

`src/components/Ghost.tsx`:
```tsx
export function Ghost({ size = 22, alert = false }: { size?: number; alert?: boolean }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" className={alert ? "ghost alert" : "ghost"} aria-label="Boo">
      <path d="M32 6C20 6 12 15 12 27v31l5-4 5 4 5-4 5 4 5-4 5 4 5-4 5 4V27C52 15 44 6 32 6z" fill="#f4f1ff" />
      <ellipse cx="25" cy="28" rx="3.5" ry="5" fill="#16131f" />
      <ellipse cx="39" cy="28" rx="3.5" ry="5" fill="#16131f" />
    </svg>
  );
}
```
`src/components/Pill.tsx`:
```tsx
import { Ghost } from "./Ghost";

export function Pill({ active, waiting, onHover }: { active: number; waiting: number; onHover: () => void }) {
  return (
    <div className="pill" onMouseEnter={onHover} data-tauri-drag-region>
      <Ghost alert={waiting > 0} />
      <span className="pill-text">{waiting > 0 ? `${waiting} waiting` : active > 0 ? `${active} active` : "Peekaboo"}</span>
    </div>
  );
}
```
`src/components/SessionList.tsx`:
```tsx
import type { Session, SessionState } from "../types";

const LABEL: Record<SessionState, string> = {
  working: "Working", waitingApproval: "Waiting for you", waitingAnswer: "Waiting for you", idle: "Idle", ended: "Ended",
};

export function SessionList({ sessions }: { sessions: Session[] }) {
  if (sessions.length === 0) return <p className="empty">No active agents. Start Claude Code and Boo will peek in.</p>;
  return (
    <ul className="sessions">
      {sessions.map((s) => (
        <li key={s.key} className={`session ${s.state}`}>
          <div className="row"><strong>{s.project}</strong><span className="badge">{LABEL[s.state]}</span></div>
          <div className="meta"><span className="agent">{s.agent}</span>{s.lastAction && <span>{s.lastAction}</span>}</div>
        </li>
      ))}
    </ul>
  );
}
```
`src/components/ApprovalCard.tsx`:
```tsx
import type { Decision, PendingView } from "../types";

type Permission = Extract<PendingView, { kind: "permission" }>;

function detail(input: Permission["input"]): string {
  if (!input) return "";
  for (const k of ["command", "file_path", "path", "url", "pattern"]) {
    if (typeof input[k] === "string") return input[k] as string;
  }
  return JSON.stringify(input).slice(0, 200);
}

export function ApprovalCard({ req, onDecide }: { req: Permission; onDecide: (d: Decision) => void }) {
  return (
    <div className="card approval">
      <div className="row"><span className="agent">{req.agent} · {req.project}</span></div>
      <div className="tool">{req.tool}</div>
      <code className="detail">{detail(req.input)}</code>
      <div className="actions">
        <button className="primary" onClick={() => onDecide({ type: "allow", always: false })}>Allow</button>
        <button onClick={() => onDecide({ type: "allow", always: true })}>Always</button>
        <button className="danger" onClick={() => onDecide({ type: "deny" })}>Deny</button>
        <button className="ghost-btn" onClick={() => onDecide({ type: "passthrough" })}>Terminal</button>
      </div>
    </div>
  );
}
```
`src/components/QuestionCard.tsx`:
```tsx
import { useState } from "react";
import type { Decision, PendingView } from "../types";

type QuestionReq = Extract<PendingView, { kind: "question" }>;

export function QuestionCard({ req, onDecide }: { req: QuestionReq; onDecide: (d: Decision) => void }) {
  const [picked, setPicked] = useState<string[][]>(req.questions.map(() => []));
  const [other, setOther] = useState<string[]>(req.questions.map(() => ""));

  const toggle = (qi: number, label: string, multi: boolean) =>
    setPicked((p) => p.map((sel, i) => (i !== qi ? sel : multi ? (sel.includes(label) ? sel.filter((l) => l !== label) : [...sel, label]) : [label])));

  const answers = req.questions.map((q, i) => ({ question: q.question, answer: other[i].trim() || picked[i].join(", ") }));
  const complete = answers.every((a) => a.answer.length > 0);

  return (
    <div className="card question">
      <div className="agent">{req.agent} · {req.project}</div>
      {req.questions.map((q, qi) => (
        <fieldset key={qi}>
          <legend>{q.question}</legend>
          <div className="options">
            {q.options.map((o) => (
              <button key={o.label} title={o.description ?? undefined} aria-pressed={picked[qi].includes(o.label)}
                className={picked[qi].includes(o.label) ? "chip on" : "chip"} onClick={() => toggle(qi, o.label, q.multiSelect)}>
                {o.label}
              </button>
            ))}
          </div>
          <input placeholder="Other…" value={other[qi]} onChange={(e) => setOther((v) => v.map((x, i) => (i === qi ? e.target.value : x)))} />
        </fieldset>
      ))}
      <div className="actions">
        <button className="primary" disabled={!complete} onClick={() => onDecide({ type: "answer", answers })}>Send</button>
        <button className="ghost-btn" onClick={() => onDecide({ type: "passthrough" })}>Answer in terminal</button>
      </div>
    </div>
  );
}
```
`src/components/SettingsPanel.tsx`:
```tsx
import type { AlwaysRule, HookStatus } from "../types";

interface Props {
  hooks: HookStatus[];
  rules: AlwaysRule[];
  onInstall: (agent: string) => void;
  onUninstall: (agent: string) => void;
  onRemoveRule: (rule: AlwaysRule) => void;
}

export function SettingsPanel({ hooks, rules, onInstall, onUninstall, onRemoveRule }: Props) {
  return (
    <div className="settings">
      <h3>Agents</h3>
      {hooks.map((h) => (
        <div key={h.agent} className="row setting">
          <div><strong>{h.agent}</strong><div className="meta">{h.error ?? h.configPath}</div></div>
          {h.installed
            ? <button onClick={() => onUninstall(h.agent)}>Uninstall</button>
            : <button className="primary" disabled={!!h.error} onClick={() => onInstall(h.agent)}>Install</button>}
        </div>
      ))}
      <h3>Always allowed</h3>
      {rules.length === 0 && <p className="empty">Nothing yet. Choose "Always" on a request to add one.</p>}
      {rules.map((r) => (
        <div key={`${r.agent}:${r.tool}`} className="row setting">
          <span>{r.agent} · {r.tool}</span>
          <button aria-label={`Remove ${r.tool}`} onClick={() => onRemoveRule(r)}>✕</button>
        </div>
      ))}
    </div>
  );
}
```
`src/App.tsx`:
```tsx
import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "./api";
import type { AppState, Decision } from "./types";
import { Pill } from "./components/Pill";
import { SessionList } from "./components/SessionList";
import { ApprovalCard } from "./components/ApprovalCard";
import { QuestionCard } from "./components/QuestionCard";
import { SettingsPanel } from "./components/SettingsPanel";
import { Ghost } from "./components/Ghost";

const EMPTY: AppState = { sessions: [], pending: [], rules: [], hooks: [] };

export default function App() {
  const [state, setState] = useState<AppState>(EMPTY);
  const [expanded, setExpandedState] = useState(false);
  const [tab, setTab] = useState<"sessions" | "settings">("sessions");
  const lastPending = useRef(0);

  const refresh = useCallback(() => api.getState().then(setState).catch(console.error), []);
  const setExpanded = useCallback((v: boolean) => { setExpandedState(v); api.setExpanded(v).catch(console.error); }, []);

  useEffect(() => {
    refresh();
    const subs = [api.onStateChanged(refresh), api.onToggle(() => setExpandedState((v) => { api.setExpanded(!v); return !v; }))];
    return () => { subs.forEach((p) => p.then((un) => un())); };
  }, [refresh]);

  useEffect(() => {
    if (state.pending.length > lastPending.current) setExpanded(true);
    lastPending.current = state.pending.length;
  }, [state.pending.length, setExpanded]);

  const onDecide = (id: string) => (d: Decision) => api.decide(id, d).catch(console.error);
  const guard = (p: Promise<unknown>) => p.then(refresh).catch((e) => alert(String(e)));

  useEffect(() => {
    const first = state.pending[0];
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return;
      if (e.key === "Escape") setExpanded(false);
      if (first?.kind === "permission" && e.key === "a") onDecide(first.id)({ type: "allow", always: false });
      if (first?.kind === "permission" && e.key === "d") onDecide(first.id)({ type: "deny" });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [state.pending, setExpanded]);

  const waiting = state.pending.length;
  if (!expanded) return <Pill active={state.sessions.length} waiting={waiting} onHover={() => setExpanded(true)} />;

  return (
    <div className="panel" onMouseLeave={() => waiting === 0 && setExpanded(false)}>
      <header className="row">
        <Ghost size={26} alert={waiting > 0} />
        <nav>
          <button className={tab === "sessions" ? "on" : ""} onClick={() => setTab("sessions")}>Agents</button>
          <button className={tab === "settings" ? "on" : ""} onClick={() => setTab("settings")}>Settings</button>
        </nav>
      </header>
      {tab === "sessions" ? (
        <main>
          {state.pending.map((p) => p.kind === "permission"
            ? <ApprovalCard key={p.id} req={p} onDecide={onDecide(p.id)} />
            : <QuestionCard key={p.id} req={p} onDecide={onDecide(p.id)} />)}
          <SessionList sessions={state.sessions} />
        </main>
      ) : (
        <SettingsPanel hooks={state.hooks} rules={state.rules}
          onInstall={(a) => guard(api.installHooks(a))} onUninstall={(a) => guard(api.uninstallHooks(a))}
          onRemoveRule={(r) => guard(api.removeRule(r.agent, r.tool))} />
      )}
    </div>
  );
}
```
`src/styles.css`:
```css
:root { --bg: #16131f; --fg: #f4f1ff; --muted: #9a93b0; --accent: #8b7bff; --danger: #ff6b81; --card: #221d30; }
html, body { margin: 0; background: transparent; overflow: hidden; color: var(--fg); font: 13px -apple-system, system-ui, "Segoe UI", sans-serif; }
button { font: inherit; color: var(--fg); background: #2e2742; border: 0; border-radius: 8px; padding: 6px 10px; cursor: pointer; }
button:disabled { opacity: .4; cursor: default; }
button.primary { background: var(--accent); }
button.danger { background: var(--danger); }
button.ghost-btn { background: transparent; color: var(--muted); }
.row { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.pill { height: 44px; background: var(--bg); border-radius: 0 0 20px 20px; display: flex; align-items: center; justify-content: center; gap: 8px; }
.pill-text { color: var(--muted); }
.panel { height: 100vh; box-sizing: border-box; background: var(--bg); border-radius: 0 0 22px 22px; padding: 10px 14px; display: flex; flex-direction: column; gap: 10px; }
.panel main, .settings { overflow-y: auto; display: flex; flex-direction: column; gap: 8px; }
nav { display: flex; gap: 4px; } nav button { background: transparent; color: var(--muted); } nav button.on { color: var(--fg); background: #2e2742; }
.card { background: var(--card); border-radius: 12px; padding: 10px; display: flex; flex-direction: column; gap: 6px; border: 1px solid #3a3150; }
.card.approval { border-color: var(--accent); }
.tool { font-weight: 600; }
.detail { background: #0e0c14; border-radius: 6px; padding: 6px; white-space: pre-wrap; word-break: break-all; max-height: 90px; overflow: auto; }
.actions { display: flex; gap: 6px; flex-wrap: wrap; }
fieldset { border: 0; margin: 0; padding: 0; } legend { margin-bottom: 6px; }
.options { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 6px; }
.chip.on { background: var(--accent); }
input { width: 100%; box-sizing: border-box; background: #0e0c14; color: var(--fg); border: 1px solid #3a3150; border-radius: 8px; padding: 6px; }
.sessions { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 6px; }
.session { background: var(--card); border-radius: 10px; padding: 8px 10px; }
.session.waitingApproval .badge, .session.waitingAnswer .badge { color: var(--accent); }
.badge, .meta, .agent, .empty { color: var(--muted); font-size: 12px; }
.meta { display: flex; gap: 8px; }
.ghost.alert { animation: peek 0.9s ease-in-out infinite; }
@keyframes peek { 50% { transform: translateY(-3px) rotate(-6deg); } }
h3 { margin: 6px 0 2px; font-size: 12px; color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
```

- [ ] **Step 5: Run** `npm test` → PASS; `npm run build` → succeeds.

- [ ] **Step 6: Manual check.** `cargo build -p peekaboo-hook && npm run tauri dev`. In Settings click Install for claude; start `claude` in another terminal; run something needing permission; confirm the card appears, Allow continues the agent, Deny blocks it, quitting Peekaboo makes Claude Code fall back to its normal prompt.

- [ ] **Step 7: Commit** `git add -A && git commit -m "feat(ui): overlay pill, approvals, questions, sessions, settings"`

---

### Task 10: Packaging, CI, README

**Files:**
- Create: `scripts/prepare-sidecar.mjs`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `README.md`
- Modify: `src-tauri/tauri.conf.json`, `package.json`

- [ ] **Step 1: Sidecar script** `scripts/prepare-sidecar.mjs`:
```js
// Builds peekaboo-hook and places it where Tauri's externalBin expects it.
import { execSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";

const release = process.argv.includes("--release");
const triple = process.env.TAURI_ENV_TARGET_TRIPLE
  || execSync("rustc -vV").toString().match(/host: (\S+)/)[1];
const ext = triple.includes("windows") ? ".exe" : "";
execSync(`cargo build -p peekaboo-hook ${release ? "--release" : ""} --target ${triple}`, { stdio: "inherit" });
mkdirSync("src-tauri/binaries", { recursive: true });
copyFileSync(`target/${triple}/${release ? "release" : "debug"}/peekaboo-hook${ext}`, `src-tauri/binaries/peekaboo-hook-${triple}${ext}`);
console.log(`sidecar ready for ${triple}`);
```
`package.json` scripts add: `"sidecar": "node scripts/prepare-sidecar.mjs"`, `"sidecar:release": "node scripts/prepare-sidecar.mjs --release"`.
`tauri.conf.json`: `"beforeDevCommand": "npm run sidecar && npm run dev"`, `"beforeBuildCommand": "npm run sidecar:release && npm run build"`, and `"bundle": { ..., "externalBin": ["binaries/peekaboo-hook"] }`.

- [ ] **Step 2: Verify** `npm run tauri build` produces a bundle and `peekaboo-hook` sits next to the main executable (macOS: `Peekaboo.app/Contents/MacOS/peekaboo-hook`). Launch the bundle, install hooks from Settings, confirm the installed command path points inside the app bundle.

- [ ] **Step 3: CI** `.github/workflows/ci.yml`:
```yaml
name: ci
on: [push, pull_request]
jobs:
  test:
    strategy:
      matrix: { os: [macos-latest, ubuntu-22.04, windows-latest] }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-node@v4
        with: { node-version: 22 }
      - if: matrix.os == 'ubuntu-22.04'
        run: sudo apt-get update && sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
      - run: cargo test
      - run: npm ci && npm test && npm run build
```
`.github/workflows/release.yml`:
```yaml
name: release
on: { push: { tags: ["v*"] } }
jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { os: macos-latest, args: "--target universal-apple-darwin" }
          - { os: ubuntu-22.04, args: "" }
          - { os: windows-latest, args: "" }
    runs-on: ${{ matrix.os }}
    permissions: { contents: write }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: "${{ matrix.os == 'macos-latest' && 'aarch64-apple-darwin,x86_64-apple-darwin' || '' }}" }
      - uses: actions/setup-node@v4
        with: { node-version: 22 }
      - if: matrix.os == 'ubuntu-22.04'
        run: sudo apt-get update && sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
      - run: npm ci
      - uses: tauri-apps/tauri-action@v0
        env: { GITHUB_TOKEN: "${{ secrets.GITHUB_TOKEN }}" }
        with: { tagName: "v__VERSION__", releaseName: "Peekaboo v__VERSION__", releaseDraft: true, args: "${{ matrix.args }}" }
```
(Universal macOS sidecar: the script gets `TAURI_ENV_TARGET_TRIPLE=universal-apple-darwin`; extend it then to build both arch triples and `lipo -create` them into `peekaboo-hook-universal-apple-darwin`. Signing/notarization secrets are added when Devx has the certificates.)

- [ ] **Step 4: README.md** — what Peekaboo is, install, "Settings → Agents → Install", how the hook works and the never-block guarantee, dev commands (`npm i`, `npm run tauri dev`, `cargo test`, `npm test`), roadmap (Phases 2–5 from the spec).

- [ ] **Step 5: Final verification** `cargo test && npm test && npm run build` → all PASS.

- [ ] **Step 6: Commit** `git add -A && git commit -m "build: sidecar packaging, CI and release workflows, README"`
