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
