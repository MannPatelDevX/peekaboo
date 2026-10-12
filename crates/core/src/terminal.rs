//! Opens a terminal that resumes a Claude Code session (`claude --resume <id>` in its project).
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Linux,
    Windows,
    /// POSIX quoting only; used for the "copy command" text on macOS/Linux.
    Unix,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchStep {
    pub program: String,
    pub args: Vec<String>,
    /// Working directory for the launched process (keeps paths out of any command parsing).
    pub cwd: Option<String>,
    /// The launcher exits quickly (osascript, open): wait for it and fall back if it fails.
    pub waits: bool,
}

fn step(program: &str, args: &[&str]) -> LaunchStep {
    LaunchStep { program: program.into(), args: args.iter().map(|a| a.to_string()).collect(), cwd: None, waits: false }
}

fn waiting(mut s: LaunchStep) -> LaunchStep {
    s.waits = true;
    s
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Session ids are UUIDs; anything else is refused before it reaches a command line.
fn check_id(id: &str) -> Result<(), String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("invalid session id {id:?}"));
    }
    Ok(())
}

/// The command a user would type to resume.
pub fn resume_command(os: Os, cwd: &str, id: &str) -> Result<String, String> {
    check_id(id)?;
    match os {
        Os::Windows => {
            if cwd.contains('"') {
                return Err("project path contains a quote".into());
            }
            Ok(format!("cd /d \"{cwd}\" && claude --resume {id}"))
        }
        _ => Ok(format!("cd {} && claude --resume {id}", sh_quote(cwd))),
    }
}

fn login_shell() -> String {
    std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into())
}

/// Ways to open `cmd` in a terminal, tried in order until one starts.
pub fn launch_plan(os: Os, prefer_ghostty: bool, cwd: &str, cmd: &str) -> Vec<LaunchStep> {
    let shell = login_shell();
    match os {
        Os::Mac | Os::Unix => {
            let script = format!(
                "tell application \"Terminal\"\n  activate\n  do script \"{}\"\nend tell",
                cmd.replace('\\', "\\\\").replace('"', "\\\"")
            );
            let terminal = waiting(step("osascript", &["-e", &script]));
            if prefer_ghostty {
                let dir = format!("--working-directory={cwd}");
                vec![waiting(step("open", &["-na", "Ghostty", "--args", &dir, "-e", &shell, "-lc", cmd])), terminal]
            } else {
                vec![terminal]
            }
        }
        Os::Linux => vec![
            step("x-terminal-emulator", &["-e", &shell, "-lc", cmd]),
            step("gnome-terminal", &["--", &shell, "-lc", cmd]),
            step("konsole", &["-e", &shell, "-lc", cmd]),
            step("xterm", &["-e", &shell, "-lc", cmd]),
        ],
        // Windows never goes through a command string: see windows_plan.
        Os::Windows => Vec::new(),
    }
}

/// cmd.exe expands `%VAR%` and splits on `&` even inside quotes, so the project path is passed
/// as a working directory, never as text cmd parses. The id is [A-Za-z0-9-] only.
pub fn windows_plan(cwd: &str, id: &str) -> Vec<LaunchStep> {
    let mut fallback = step("cmd.exe", &["/c", "start", "", "cmd.exe", "/k", "claude", "--resume", id]);
    fallback.cwd = Some(cwd.to_string());
    vec![step("wt.exe", &["-d", cwd, "cmd.exe", "/k", "claude", "--resume", id]), fallback]
}

const LAUNCHER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Tries each step until one starts. Quick launchers are waited on so a failure (e.g. macOS
/// denying Automation access to Terminal) falls through to the next option and is reported.
/// Every child is reaped so nothing is left as a zombie.
pub fn run_plan(steps: &[LaunchStep]) -> Result<(), String> {
    let mut errors = Vec::new();
    for s in steps {
        let mut cmd = Command::new(&s.program);
        cmd.args(&s.args).stdin(Stdio::null()).stdout(Stdio::null());
        cmd.stderr(if s.waits { Stdio::piped() } else { Stdio::null() });
        if let Some(dir) = &s.cwd {
            cmd.current_dir(dir);
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("{}: {e}", s.program));
                continue;
            }
        };
        if !s.waits {
            std::thread::spawn(move || child.wait());
            return Ok(());
        }
        let deadline = std::time::Instant::now() + LAUNCHER_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    let mut msg = String::new();
                    if let Some(mut e) = child.stderr.take() {
                        let _ = std::io::Read::read_to_string(&mut e, &mut msg);
                    }
                    errors.push(format!("{} failed ({status}): {}", s.program, msg.trim()));
                    break;
                }
                Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(50)),
                // Still running after the deadline (e.g. waiting on a permission prompt): let it be.
                _ => {
                    std::thread::spawn(move || child.wait());
                    return Ok(());
                }
            }
        }
    }
    Err(if errors.is_empty() { "no terminal found".into() } else { errors.join("; ") })
}

/// AppleScript string literal body: no control characters (a newline would submit early), escaped.
fn applescript_text(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\n' | '\r' | '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect::<String>()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// AppleScript that types `text` (then Return) into the Terminal.app / iTerm2 tab on `tty`.
/// Each script is guarded by `is running`, so an app that isn't open is never launched.
/// Prints "ok" when it found the tab.
pub fn typing_scripts(tty: &str, text: &str, iterm_installed: bool) -> Vec<String> {
    let name = tty.trim_start_matches("/dev/");
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Vec::new();
    }
    let dev = format!("/dev/{name}");
    let text = applescript_text(text);
    let mut scripts = vec![format!(
        "if application \"Terminal\" is running then\n\
         tell application \"Terminal\"\n\
         repeat with w in windows\nrepeat with t in tabs of w\n\
         if tty of t is \"{dev}\" then\ndo script \"{text}\" in t\nreturn \"ok\"\nend if\n\
         end repeat\nend repeat\nend tell\nend if\nreturn \"missing\""
    )];
    if iterm_installed {
        scripts.push(format!(
            "if application \"iTerm2\" is running then\n\
             tell application \"iTerm2\"\n\
             repeat with w in windows\nrepeat with t in tabs of w\nrepeat with s in sessions of t\n\
             if tty of s is \"{dev}\" then\ntell s to write text \"{text}\"\nreturn \"ok\"\nend if\n\
             end repeat\nend repeat\nend repeat\nend tell\nend if\nreturn \"missing\""
        ));
    }
    scripts
}

/// Types `text` into the agent's terminal tab. Ok(false) when no supported terminal owns `tty`.
pub fn type_into_session(tty: &str, text: &str) -> Result<bool, String> {
    if !cfg!(target_os = "macos") {
        return Ok(false);
    }
    let iterm = std::path::Path::new("/Applications/iTerm.app").exists();
    for script in typing_scripts(tty, text, iterm) {
        let out = Command::new("osascript").args(["-e", &script]).output().map_err(|e| e.to_string())?;
        if out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "ok" {
            return Ok(true);
        }
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            // -1743: the user hasn't allowed Peekaboo to control this app.
            if err.contains("-1743") {
                return Err("Allow Peekaboo to control Terminal in System Settings → Privacy & Security → Automation".into());
            }
        }
    }
    Ok(false)
}

fn ghostty_installed() -> bool {
    cfg!(target_os = "macos")
        && (std::path::Path::new("/Applications/Ghostty.app").exists()
            || dirs::home_dir().is_some_and(|h| h.join("Applications/Ghostty.app").exists()))
}

/// Opens a terminal resuming session `id` in `cwd`.
pub fn resume_in_terminal(cwd: &str, id: &str) -> Result<(), String> {
    if !std::path::Path::new(cwd).is_dir() {
        return Err(format!("project folder no longer exists: {cwd}"));
    }
    check_id(id)?;
    let os = Os::current();
    let steps = match os {
        Os::Windows => windows_plan(cwd, id),
        _ => launch_plan(os, ghostty_installed(), cwd, &resume_command(os, cwd, id)?),
    };
    run_plan(&steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "24bd582e-3f62-428d-8eeb-7aae27829afb";

    #[test]
    fn posix_command_quotes_paths() {
        assert_eq!(resume_command(Os::Unix, "/w/my shop", ID).unwrap(), format!("cd '/w/my shop' && claude --resume {ID}"));
        assert_eq!(resume_command(Os::Unix, "/w/it's", ID).unwrap(), format!("cd '/w/it'\\''s' && claude --resume {ID}"));
    }

    #[test]
    fn windows_command() {
        assert_eq!(resume_command(Os::Windows, r"C:\w\shop", ID).unwrap(), format!("cd /d \"C:\\w\\shop\" && claude --resume {ID}"));
        assert!(resume_command(Os::Windows, "C:\\w\\a\"b", ID).is_err());
    }

    #[test]
    fn rejects_non_uuid_ids() {
        for bad in ["", "abc; rm -rf ~", "../x", "$(id)"] {
            assert!(resume_command(Os::Unix, "/w", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn mac_plans() {
        let cmd = "cd '/w/\"q\"' && claude --resume x";
        let terminal = launch_plan(Os::Mac, false, "/w", cmd);
        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0].program, "osascript");
        assert!(terminal[0].args[1].contains(r#"do script "cd '/w/\"q\"' && claude --resume x""#), "{:?}", terminal[0].args);
        let ghostty = launch_plan(Os::Mac, true, "/w", cmd);
        assert_eq!(ghostty[0].program, "open");
        assert_eq!(ghostty[0].args[..3], ["-na", "Ghostty", "--args"]);
        assert_eq!(ghostty[1].program, "osascript", "falls back to Terminal");
    }

    #[test]
    fn linux_and_windows_plans_have_fallbacks() {
        let l = launch_plan(Os::Linux, false, "/w", "cd '/w' && claude --resume x");
        assert_eq!(l.iter().map(|s| s.program.as_str()).collect::<Vec<_>>(), ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"]);
        assert!(l.iter().all(|s| !s.waits), "terminal emulators stay in the foreground");
    }

    #[test]
    fn windows_keeps_the_path_out_of_cmd_parsing() {
        let w = windows_plan(r"C:\w\100%done & co", ID);
        assert_eq!(w[0].program, "wt.exe");
        assert_eq!(w[0].args, ["-d", r"C:\w\100%done & co", "cmd.exe", "/k", "claude", "--resume", ID]);
        assert_eq!(w[1].program, "cmd.exe");
        assert_eq!(w[1].cwd.as_deref(), Some(r"C:\w\100%done & co"));
        assert!(w[1].args.iter().all(|a| !a.contains("C:")), "{:?}", w[1].args);
    }

    #[cfg(unix)]
    #[test]
    fn failed_launcher_falls_back_and_errors_are_reported() {
        let ok = |program: &str| LaunchStep { program: program.into(), args: vec![], cwd: None, waits: true };
        assert!(run_plan(&[ok("false"), ok("true")]).is_ok(), "falls back after a launcher exits non-zero");
        let err = run_plan(&[ok("false"), ok("/no/such/program")]).unwrap_err();
        assert!(err.contains("/no/such/program"), "{err}");
    }

    #[test]
    fn typing_scripts_target_the_tab_by_tty_and_never_launch_apps() {
        let scripts = typing_scripts("ttys004", "fix \"it\" \\ now\nplease", true);
        assert_eq!(scripts.len(), 2);
        let terminal = &scripts[0];
        assert!(terminal.starts_with("if application \"Terminal\" is running then"), "{terminal}");
        assert!(terminal.contains("tty of t is \"/dev/ttys004\""));
        assert!(terminal.contains(r#"do script "fix \"it\" \\ now please" in t"#), "{terminal}");
        assert!(scripts[1].contains("application \"iTerm2\" is running"));
        assert!(scripts[1].contains(r#"write text "fix \"it\" \\ now please""#));
        assert_eq!(typing_scripts("/dev/ttys004", "x", false).len(), 1, "no iTerm2 script unless installed");
        assert!(typing_scripts("/dev/ttys004", "x", false)[0].contains("\"/dev/ttys004\""));
    }

    #[test]
    fn typing_rejects_odd_ttys_and_strips_control_characters() {
        assert!(typing_scripts("ttys004\" & do shell script \"x", "hi", false).is_empty());
        assert!(typing_scripts("", "hi", false).is_empty());
        assert!(typing_scripts("ttys1", "a\u{1b}[2Jb\tc", false)[0].contains(r#"do script "a[2Jb c" in t"#));
    }
}
