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
fn run(bin: &Path, args: &[String], timeout: Duration) -> Result<(String, bool), String> {
    let mut child = Command::new(bin)
        .args(args)
        .env("LANG", "en_US.UTF-8")
        .env("LC_ALL", "en_US.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run claudex: {e}"))?;
    let drain = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = r.read_to_string(&mut s);
            s
        })
    };
    let out = drain(Box::new(child.stdout.take().expect("piped")));
    let err = drain(Box::new(child.stderr.take().expect("piped")));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("claudex {} timed out after {}s", args.join(" "), timeout.as_secs()));
            }
        }
    };
    let text = out.join().unwrap_or_default() + &err.join().unwrap_or_default();
    Ok((text, status.success()))
}

pub struct Claudex {
    bin: Option<PathBuf>,
    pub read_timeout: Duration,
    cache: Mutex<Option<(Instant, ClaudexSnapshot)>>,
}

impl Claudex {
    pub fn new(bin: Option<PathBuf>) -> Claudex {
        Claudex { bin, read_timeout: READ_TIMEOUT, cache: Mutex::new(None) }
    }

    pub fn snapshot(&self, force: bool) -> ClaudexSnapshot {
        let mut cache = self.cache.lock().unwrap();
        if let Some((at, snap)) = cache.as_ref() {
            if !force && at.elapsed() < CACHE_TTL {
                return snap.clone();
            }
        }
        let snap = self.capture();
        *cache = Some((Instant::now(), snap.clone()));
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
             'switch '*) echo \"switched to $2\";;\n*) echo \"error: unrecognized arguments\"; exit 2;;\nesac\n",
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
        let err = c.act(&Action::Pool { on: true }).unwrap_err();
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
}
