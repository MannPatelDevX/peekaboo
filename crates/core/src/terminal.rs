//! Opens a terminal that resumes a Claude Code session (`claude --resume <id>` in its project).
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};

/// Terminal app Peekaboo opens Claude sessions in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenIn {
    #[default]
    Terminal,
    #[serde(rename = "iterm2")]
    ITerm2,
    Warp,
    Ghostty,
    Kitty,
    Alacritty,
    #[serde(rename = "wezterm")]
    WezTerm,
}

impl OpenIn {
    pub const ALL: [OpenIn; 7] =
        [OpenIn::Terminal, OpenIn::ITerm2, OpenIn::Warp, OpenIn::Ghostty, OpenIn::Kitty, OpenIn::Alacritty, OpenIn::WezTerm];

    pub fn name(self) -> &'static str {
        match self {
            OpenIn::Terminal => "Terminal",
            OpenIn::ITerm2 => "iTerm2",
            OpenIn::Warp => "Warp",
            OpenIn::Ghostty => "Ghostty",
            OpenIn::Kitty => "kitty",
            OpenIn::Alacritty => "Alacritty",
            OpenIn::WezTerm => "WezTerm",
        }
    }

    fn bundle_id(self) -> &'static str {
        match self {
            OpenIn::Terminal => "com.apple.Terminal",
            OpenIn::ITerm2 => "com.googlecode.iterm2",
            OpenIn::Warp => "dev.warp.Warp-Stable",
            OpenIn::Ghostty => "com.mitchellh.ghostty",
            OpenIn::Kitty => "net.kovidgoyal.kitty",
            OpenIn::Alacritty => "org.alacritty",
            OpenIn::WezTerm => "com.github.wez.wezterm",
        }
    }

    /// Peekaboo can type into it, so messages arrive as real user prompts.
    pub fn real_prompts(self) -> bool {
        matches!(self, OpenIn::Terminal | OpenIn::ITerm2)
    }

    /// The app can be told to run `claude --resume` when it opens. Warp can't: Peekaboo opens a
    /// tab in the project and puts the command on the clipboard.
    pub fn runs_command(self) -> bool {
        self != OpenIn::Warp
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalApp {
    pub id: OpenIn,
    pub name: &'static str,
    pub real_prompts: bool,
    pub runs_command: bool,
}

/// Installed terminal apps, found by bundle id through `find(bundle_id) -> path`.
pub fn detect_terminals_with(find: impl Fn(&str) -> Option<String>) -> Vec<TerminalApp> {
    OpenIn::ALL
        .into_iter()
        .filter(|app| *app == OpenIn::Terminal || find(app.bundle_id()).is_some())
        .map(|id| TerminalApp { id, name: id.name(), real_prompts: id.real_prompts(), runs_command: id.runs_command() })
        .collect()
}

/// Installed terminal apps on this Mac (Spotlight lookup by bundle id). Empty elsewhere.
pub fn detect_terminals() -> Vec<TerminalApp> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    detect_terminals_with(|bundle| {
        let out = Command::new("mdfind").arg(format!("kMDItemCFBundleIdentifier == '{bundle}'")).output().ok()?;
        String::from_utf8_lossy(&out.stdout).lines().next().map(str::to_string).filter(|p| !p.is_empty())
    })
}

pub fn installed(app: OpenIn) -> bool {
    detect_terminals().iter().any(|a| a.id == app)
}

pub fn iterm_installed() -> bool {
    installed(OpenIn::ITerm2)
}

/// What the user still has to do after a resume.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ResumeOutcome {
    /// The session is starting in the chosen app.
    Started { app: &'static str },
    /// The app opened in the project; the resume command is on the clipboard to paste.
    PasteCommand { app: &'static str, command: String },
}

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
pub fn launch_plan(os: Os, open_in: OpenIn, cwd: &str, cmd: &str) -> Vec<LaunchStep> {
    let shell = login_shell();
    match os {
        Os::Mac | Os::Unix => {
            let quoted = cmd.replace('\\', "\\\\").replace('"', "\\\"");
            let script = format!("tell application \"Terminal\"\n  activate\n  do script \"{quoted}\"\nend tell");
            let terminal = waiting(step("osascript", &["-e", &script]));
            let sh = shell.as_str();
            let first = match open_in {
                OpenIn::Terminal => return vec![terminal],
                OpenIn::ITerm2 => step("osascript", &["-e", &format!(
                    "tell application \"iTerm2\"\n  activate\n  set w to (create window with default profile)\n  \
                     tell current session of w to write text \"{quoted}\"\nend tell"
                )]),
                OpenIn::Warp => step("open", &["-a", "Warp", cwd]),
                OpenIn::Ghostty => step("open", &["-na", "Ghostty", "--args", &format!("--working-directory={cwd}"), "-e", sh, "-lc", cmd]),
                OpenIn::Kitty => step("open", &["-na", "kitty", "--args", "--directory", cwd, sh, "-lc", cmd]),
                OpenIn::Alacritty => step("open", &["-na", "Alacritty", "--args", "--working-directory", cwd, "-e", sh, "-lc", cmd]),
                OpenIn::WezTerm => step("open", &["-na", "WezTerm", "--args", "start", "--cwd", cwd, "--", sh, "-lc", cmd]),
            };
            vec![waiting(first), terminal]
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
    for script in typing_scripts(tty, text, iterm_installed()) {
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

/// Opens a terminal resuming session `id` in `cwd`.
pub fn resume_in_terminal(cwd: &str, id: &str, open_in: OpenIn) -> Result<ResumeOutcome, String> {
    if !std::path::Path::new(cwd).is_dir() {
        return Err(format!("project folder no longer exists: {cwd}"));
    }
    check_id(id)?;
    let os = Os::current();
    if os == Os::Windows {
        run_plan(&windows_plan(cwd, id))?;
        return Ok(ResumeOutcome::Started { app: "Windows Terminal" });
    }
    // Never script an app that isn't installed: AppleScript would pop up "Where is …?".
    let app = if open_in == OpenIn::Terminal || installed(open_in) { open_in } else { OpenIn::Terminal };
    let cmd = resume_command(os, cwd, id)?;
    run_plan(&launch_plan(os, app, cwd, &cmd))?;
    if app.runs_command() {
        return Ok(ResumeOutcome::Started { app: app.name() });
    }
    copy_to_clipboard(&cmd)?;
    Ok(ResumeOutcome::PasteCommand { app: app.name(), command: cmd })
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut child = Command::new("pbcopy").stdin(Stdio::piped()).spawn().map_err(|e| format!("pbcopy: {e}"))?;
    child.stdin.take().expect("piped").write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    child.wait().map_err(|e| e.to_string())?;
    Ok(())
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
        let terminal = launch_plan(Os::Mac, OpenIn::Terminal, "/w", cmd);
        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0].program, "osascript");
        assert!(terminal[0].args[1].contains(r#"do script "cd '/w/\"q\"' && claude --resume x""#), "{:?}", terminal[0].args);
        let iterm = launch_plan(Os::Mac, OpenIn::ITerm2, "/w", cmd);
        assert_eq!(iterm[0].program, "osascript");
        assert!(iterm[0].args[1].contains("tell application \"iTerm2\""), "{:?}", iterm[0].args);
        assert!(iterm[0].args[1].contains(r#"write text "cd '/w/\"q\"' && claude --resume x""#));
        assert!(iterm[1].args[1].contains("tell application \"Terminal\""), "falls back to Terminal");
    }

    #[test]
    fn linux_and_windows_plans_have_fallbacks() {
        let l = launch_plan(Os::Linux, OpenIn::Terminal, "/w", "cd '/w' && claude --resume x");
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

    #[test]
    fn detects_installed_terminals_and_their_abilities() {
        let found = |bundle: &str| matches!(bundle, "dev.warp.Warp-Stable" | "com.mitchellh.ghostty").then(|| "/Applications/X.app".to_string());
        let apps = detect_terminals_with(found);
        let ids: Vec<_> = apps.iter().map(|a| a.id).collect();
        assert_eq!(ids, [OpenIn::Terminal, OpenIn::Warp, OpenIn::Ghostty], "Terminal is always there on macOS");
        let warp = &apps[1];
        assert!(!warp.real_prompts && !warp.runs_command);
        assert!(apps[0].real_prompts && apps[0].runs_command);
        assert!(apps[2].runs_command && !apps[2].real_prompts);
    }

    #[test]
    fn launch_plans_for_each_app_fall_back_to_terminal() {
        let cmd = "cd '/w' && claude --resume x";
        for (app, program, first_args) in [
            (OpenIn::Ghostty, "open", vec!["-na", "Ghostty", "--args", "--working-directory=/w", "-e"]),
            (OpenIn::Kitty, "open", vec!["-na", "kitty", "--args", "--directory", "/w"]),
            (OpenIn::Alacritty, "open", vec!["-na", "Alacritty", "--args", "--working-directory", "/w", "-e"]),
            (OpenIn::WezTerm, "open", vec!["-na", "WezTerm", "--args", "start", "--cwd", "/w", "--"]),
            (OpenIn::Warp, "open", vec!["-a", "Warp", "/w"]),
        ] {
            let plan = launch_plan(Os::Mac, app, "/w", cmd);
            assert_eq!(plan[0].program, program, "{app:?}");
            assert_eq!(plan[0].args[..first_args.len()], first_args[..], "{app:?}");
            assert!(plan.last().unwrap().args[1].contains("tell application \"Terminal\""), "{app:?} falls back");
        }
        let ghostty = launch_plan(Os::Mac, OpenIn::Ghostty, "/w", cmd);
        assert_eq!(ghostty[0].args.last().unwrap(), cmd, "the resume command runs in the new window");
    }
}
