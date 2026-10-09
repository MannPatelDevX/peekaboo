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
