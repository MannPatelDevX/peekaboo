//! Drives the `claudex` CLI for the Accounts tab. Safety rules follow claudex-dash
//! (MIT, © 2026 Aditya Thummar):
//! - every argv is fixed in code and run without a shell;
//! - the only user-supplied value, an account name for `switch`, must appear in `claudex list`;
//! - reads time out after 10 s and are cached 60 s; actions time out after 30 s and clear the cache.
//!
//! Never reachable from here: login, add, rename, remove, refresh, update, keep-warm,
//! pool join/use, access (any), sessions share/pull, autoswitch run.
pub mod parse;

use parse::*;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const READ_TIMEOUT: Duration = Duration::from_secs(10);
const ACTION_TIMEOUT: Duration = Duration::from_secs(30);
const CACHE_TTL: Duration = Duration::from_secs(60);
pub const INSTALL_URL: &str = "https://github.com/aditya-thummar-devx/claudex-dash#what-you-need-first";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    Switch { account: String },
    Pool { on: bool },
    Autoswitch { on: bool },
}

/// The exact argv for `action`. `known_accounts` must come from `claudex list`.
pub fn argv_for(action: &Action, known_accounts: &[String]) -> Result<Vec<String>, String> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
    Ok(match action {
        Action::Switch { account } => {
            if account.is_empty() || account.starts_with('-') || !known_accounts.contains(account) {
                return Err(format!("\"{account}\" is not one of your saved claudex accounts"));
            }
            vec!["switch".into(), account.clone(), "--force".into()]
        }
        Action::Pool { on } => v(&["pool", if *on { "start" } else { "stop" }]),
        Action::Autoswitch { on } => v(&["autoswitch", if *on { "on" } else { "off" }]),
    })
}

/// `$CLAUDEX_BIN`, then where claudex installs itself, then `PATH`.
pub fn find_claudex() -> Option<PathBuf> {
    let exe = format!("claudex{}", std::env::consts::EXE_SUFFIX);
    let mut candidates: Vec<PathBuf> = std::env::var_os("CLAUDEX_BIN").map(PathBuf::from).into_iter().collect();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin").join(&exe));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin").join(&exe));
    candidates.push(PathBuf::from("/usr/local/bin").join(&exe));
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join(&exe)));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// One section of the snapshot: parsed data when the output was recognised, always the raw text.
#[derive(Debug, Clone, Serialize)]
pub struct Section<T> {
    pub data: Option<T>,
    pub raw: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudexSnapshot {
    pub installed: bool,
    pub install_url: &'static str,
    pub usage: Section<Vec<UsageRow>>,
    pub accounts: Section<Vec<AccountRow>>,
    pub current: Section<Current>,
    pub pool: Section<PoolStatus>,
    pub autoswitch: Option<bool>,
}

fn section<T>(out: &Result<String, String>, parse: impl Fn(&str) -> Option<T>) -> Section<T> {
    match out {
        Ok(raw) => Section { data: parse(raw), raw: raw.clone(), error: None },
        Err(e) => Section { data: None, raw: String::new(), error: Some(e.clone()) },
    }
}

/// Runs claudex without a shell; on timeout the process is killed. Ok carries stdout+stderr
/// whatever the exit code (claudex exits non-zero on output that is still parseable).
/// GUI apps start with a near-empty PATH; claudex shells out to `claude` and friends.
fn child_path(bin: &Path) -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = bin.parent().map(Path::to_path_buf).into_iter().collect();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/bin"));
    }
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    if let Some(existing) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

/// After claudex exits, how long to wait for its pipes to close. A background process it started
/// can inherit them and keep them open indefinitely; we take what has been written by then.
const PIPE_GRACE: Duration = Duration::from_millis(300);

/// Reads a pipe into a shared buffer; signals on `done` at EOF.
fn drain(mut r: impl Read + Send + 'static, done: std::sync::mpsc::Sender<()>) -> std::sync::Arc<Mutex<Vec<u8>>> {
    let buf = std::sync::Arc::new(Mutex::new(Vec::new()));
    let sink = buf.clone();
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = r.read(&mut chunk) {
            if n == 0 {
                break;
            }
            sink.lock().unwrap().extend_from_slice(&chunk[..n]);
        }
        let _ = done.send(());
    });
    buf
}

/// Kills claudex and anything it started (it runs in its own process group on Unix).
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    // SAFETY: plain syscall on a pid we spawned as a process-group leader.
    unsafe {
        libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn run(bin: &Path, args: &[String], timeout: Duration) -> Result<(String, bool), String> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env("PATH", child_path(bin))
        .env("LANG", "en_US.UTF-8")
        .env("LC_ALL", "en_US.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().map_err(|e| format!("could not run claudex: {e}"))?;
    let (tx, rx) = std::sync::mpsc::channel();
    let out = drain(child.stdout.take().expect("piped"), tx.clone());
    let err = drain(child.stderr.take().expect("piped"), tx);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                kill_tree(&mut child);
                return Err(format!("claudex {} timed out after {}s", args.join(" "), timeout.as_secs()));
            }
        }
    };
    let grace = Instant::now() + PIPE_GRACE;
    for _ in 0..2 {
        if rx.recv_timeout(grace.saturating_duration_since(Instant::now())).is_err() {
            break;
        }
    }
    let text = |b: &std::sync::Arc<Mutex<Vec<u8>>>| String::from_utf8_lossy(&b.lock().unwrap()).into_owned();
    Ok((text(&out) + &text(&err), status.success()))
}

pub struct Claudex {
    bin: Option<PathBuf>,
    pub read_timeout: Duration,
    cache: Mutex<Option<(Instant, ClaudexSnapshot)>>,
    /// Actions change account state; two at once would race.
    acting: Mutex<()>,
}

impl Claudex {
    pub fn new(bin: Option<PathBuf>) -> Claudex {
        Claudex { bin, read_timeout: READ_TIMEOUT, cache: Mutex::new(None), acting: Mutex::new(()) }
    }

    pub fn snapshot(&self, force: bool) -> ClaudexSnapshot {
        if !force {
            if let Some((at, snap)) = self.cache.lock().unwrap().as_ref() {
                if at.elapsed() < CACHE_TTL {
                    return snap.clone();
                }
            }
        }
        // Captured without holding the lock: a slow claudex must not block cached readers.
        let snap = self.capture();
        *self.cache.lock().unwrap() = Some((Instant::now(), snap.clone()));
        snap
    }

    fn capture(&self) -> ClaudexSnapshot {
        let read = |args: &[&str]| -> Result<String, String> {
            let Some(bin) = &self.bin else { return Err("claudex is not installed".into()) };
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            run(bin, &args, self.read_timeout).map(|(text, _)| text)
        };
        // `usage` alone takes over a second; run the reads side by side.
        let (usage, list, current, pool, doctor) = std::thread::scope(|s| {
            let usage = s.spawn(|| read(&["usage"]));
            let list = s.spawn(|| read(&["list"]));
            let current = s.spawn(|| read(&["current"]));
            let pool = s.spawn(|| read(&["pool", "status"]));
            let doctor = s.spawn(|| read(&["doctor"]));
            let j = |h: std::thread::ScopedJoinHandle<'_, Result<String, String>>| {
                h.join().unwrap_or_else(|_| Err("claudex read panicked".into()))
            };
            (j(usage), j(list), j(current), j(pool), j(doctor))
        });
        ClaudexSnapshot {
            installed: self.bin.is_some(),
            install_url: INSTALL_URL,
            usage: section(&usage, parse_usage),
            accounts: section(&list, parse_list),
            current: section(&current, parse_current),
            pool: section(&pool, parse_pool_status),
            autoswitch: doctor.ok().and_then(|d| parse_autoswitch(&d)),
        }
    }

    /// Runs one allowed action. Returns claudex's output on success.
    pub fn act(&self, action: &Action) -> Result<String, String> {
        let bin = self.bin.as_ref().ok_or("claudex is not installed")?;
        let _one_at_a_time = self.acting.lock().unwrap_or_else(|e| e.into_inner());
        let known: Vec<String> = match action {
            Action::Switch { .. } => self
                .snapshot(false)
                .accounts
                .data
                .ok_or("could not read your claudex account list")?
                .into_iter()
                .map(|a| a.account)
                .collect(),
            _ => Vec::new(),
        };
        let argv = argv_for(action, &known)?;
        let result = run(bin, &argv, ACTION_TIMEOUT);
        *self.cache.lock().unwrap() = None;
        match result? {
            (text, true) => Ok(text.trim().to_string()),
            (text, false) => Err(text.trim().to_string()),
        }
    }
}


#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn known() -> Vec<String> {
        vec!["alice".into(), "brian".into()]
    }

    #[test]
    fn argv_is_fixed_per_action() {
        assert_eq!(argv_for(&Action::Switch { account: "alice".into() }, &known()).unwrap(), ["switch", "alice", "--force"]);
        assert_eq!(argv_for(&Action::Pool { on: true }, &known()).unwrap(), ["pool", "start"]);
        assert_eq!(argv_for(&Action::Pool { on: false }, &known()).unwrap(), ["pool", "stop"]);
        assert_eq!(argv_for(&Action::Autoswitch { on: true }, &known()).unwrap(), ["autoswitch", "on"]);
        assert_eq!(argv_for(&Action::Autoswitch { on: false }, &known()).unwrap(), ["autoswitch", "off"]);
    }

    #[test]
    fn switch_only_accepts_names_claudex_listed() {
        for bad in ["mallory", "", "--help", "alice; rm -rf ~", "alice --other"] {
            assert!(argv_for(&Action::Switch { account: bad.into() }, &known()).is_err(), "{bad}");
        }
        let flaggy = vec!["--force".to_string()];
        assert!(argv_for(&Action::Switch { account: "--force".into() }, &flaggy).is_err());
    }

    /// A fake `claudex` that prints fixtures and counts invocations.
    fn fake(dir: &Path, body: &str) -> PathBuf {
        let fx = format!("{}/tests/fixtures/claudex", env!("CARGO_MANIFEST_DIR"));
        let p = dir.join("claudex");
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> \"{log}\"\n{body}\ncase \"$1 $2\" in\n\
             'usage '*) cat {fx}/usage.txt;;\n'list '*) cat {fx}/list.txt;;\n'current '*) cat {fx}/current.txt;;\n\
             'pool status') cat {fx}/pool-status.txt;;\n'doctor '*) cat {fx}/doctor.txt;;\n\
             'switch '*) echo \"switched to $2\";;\n\
             'pool start') echo begin >> \"{log}\"; sleep 0.3; echo end >> \"{log}\"; echo started;;\n\
             'pool stop') (sleep 30) & echo stopped;;\n*) echo \"error: unrecognized arguments\"; exit 2;;\nesac\n",
            log = dir.join("calls.log").display()
        );
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn calls(dir: &Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("calls.log")).unwrap_or_default().lines().map(str::to_string).collect()
    }

    #[test]
    fn snapshot_parses_everything_and_caches() {
        let d = tempfile::tempdir().unwrap();
        let c = Claudex::new(Some(fake(d.path(), "")));
        let s = c.snapshot(false);
        assert!(s.installed);
        assert_eq!(s.usage.data.as_ref().unwrap().len(), 4);
        assert_eq!(s.accounts.data.as_ref().unwrap()[0].account, "alice");
        assert_eq!(s.current.data.as_ref().unwrap().account, "brian");
        assert!(!s.pool.data.as_ref().unwrap().consuming.on);
        assert_eq!(s.autoswitch, Some(false));
        let n = calls(d.path()).len();
        assert_eq!(n, 5);
        c.snapshot(false);
        assert_eq!(calls(d.path()).len(), n, "cached");
        c.snapshot(true);
        assert_eq!(calls(d.path()).len(), 2 * n, "forced refresh");
    }

    #[test]
    fn act_runs_validated_command_and_invalidates_cache() {
        let d = tempfile::tempdir().unwrap();
        let c = Claudex::new(Some(fake(d.path(), "")));
        assert!(c.act(&Action::Switch { account: "mallory".into() }).is_err());
        assert!(!calls(d.path()).iter().any(|l| l.starts_with("switch")), "unknown name never reaches claudex");
        assert_eq!(c.act(&Action::Switch { account: "alice".into() }).unwrap(), "switched to alice");
        assert!(calls(d.path()).contains(&"switch alice --force".to_string()));
        let before = calls(d.path()).len();
        c.snapshot(false);
        assert!(calls(d.path()).len() > before, "cache invalidated after an action");
    }

    #[test]
    fn failing_action_reports_claudex_output() {
        let d = tempfile::tempdir().unwrap();
        let c = Claudex::new(Some(fake(d.path(), "")));
        let err = c.act(&Action::Autoswitch { on: true }).unwrap_err();
        assert!(err.contains("unrecognized arguments"), "{err}");
    }

    #[test]
    fn hung_claudex_times_out() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Claudex::new(Some(fake(d.path(), "sleep 5")));
        c.read_timeout = std::time::Duration::from_millis(300);
        let t = std::time::Instant::now();
        let s = c.snapshot(true);
        assert!(t.elapsed() < std::time::Duration::from_secs(3));
        assert!(s.usage.data.is_none());
        assert!(s.usage.error.as_deref().unwrap().contains("timed out"));
    }

    #[test]
    fn missing_claudex_is_reported_not_run() {
        let c = Claudex::new(None);
        let s = c.snapshot(true);
        assert!(!s.installed);
        assert!(c.act(&Action::Pool { on: true }).is_err());
    }

    #[test]
    fn lingering_grandchild_holding_the_pipe_does_not_hang() {
        // `pool stop` in the fake leaves a background child that inherits stdout and outlives claudex.
        let d = tempfile::tempdir().unwrap();
        let c = Claudex::new(Some(fake(d.path(), "")));
        let t = std::time::Instant::now();
        assert_eq!(c.act(&Action::Pool { on: false }).unwrap(), "stopped");
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
    }

    #[test]
    fn actions_run_one_at_a_time() {
        let d = tempfile::tempdir().unwrap();
        let c = std::sync::Arc::new(Claudex::new(Some(fake(d.path(), ""))));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let c = c.clone();
                std::thread::spawn(move || c.act(&Action::Pool { on: true }))
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap().unwrap(), "started");
        }
        let marks: Vec<String> = calls(d.path()).into_iter().filter(|l| l == "begin" || l == "end").collect();
        assert_eq!(marks, ["begin", "end", "begin", "end"]);
    }

    #[test]
    fn slow_capture_does_not_block_cached_readers() {
        let d = tempfile::tempdir().unwrap();
        let c = std::sync::Arc::new(Claudex::new(Some(fake(d.path(), "sleep 0.4"))));
        c.snapshot(false);
        let slow = {
            let c = c.clone();
            std::thread::spawn(move || c.snapshot(true))
        };
        std::thread::sleep(std::time::Duration::from_millis(20));
        let t = std::time::Instant::now();
        c.snapshot(false);
        assert!(t.elapsed() < std::time::Duration::from_millis(50), "cached read waited {:?}", t.elapsed());
        slow.join().unwrap();
    }
}
