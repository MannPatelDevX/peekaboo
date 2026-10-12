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
}

fn step(program: &str, args: &[&str]) -> LaunchStep {
    LaunchStep { program: program.into(), args: args.iter().map(|a| a.to_string()).collect() }
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The command a user would type to resume. Session ids are UUIDs; anything else is refused.
pub fn resume_command(os: Os, cwd: &str, id: &str) -> Result<String, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("invalid session id {id:?}"));
    }
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
            let terminal = step("osascript", &["-e", &script]);
            if prefer_ghostty {
                let dir = format!("--working-directory={cwd}");
                vec![step("open", &["-na", "Ghostty", "--args", &dir, "-e", &shell, "-lc", cmd]), terminal]
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
        Os::Windows => vec![
            step("wt.exe", &["-d", cwd, "cmd.exe", "/k", cmd]),
            step("cmd.exe", &["/c", "start", "", "cmd.exe", "/k", cmd]),
        ],
    }
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
    let os = Os::current();
    let cmd = resume_command(os, cwd, id)?;
    let mut last_err = String::from("no terminal found");
    for s in launch_plan(os, ghostty_installed(), cwd, &cmd) {
        let spawned = Command::new(&s.program)
            .args(&s.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            Ok(_) => return Ok(()),
            Err(e) => last_err = format!("{}: {e}", s.program),
        }
    }
    Err(last_err)
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
        let w = launch_plan(Os::Windows, false, r"C:\w", "cd /d \"C:\\w\" && claude --resume x");
        assert_eq!(w[0].program, "wt.exe");
        assert_eq!(w[1].program, "cmd.exe");
    }
}
