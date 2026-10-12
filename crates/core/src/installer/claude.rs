use super::*;
use serde_json::json;

#[derive(Clone, Copy)]
enum Wait {
    No,
    /// Uses the configured approval timeout.
    Approval,
    /// Questions block Claude's own terminal prompt, so they give up quickly.
    Question,
    /// Stop is answered at once (delivering any prompt queued in Peekaboo); the cap is a safety net.
    Stop,
}

const EVENTS: &[(&str, Option<&str>, Wait)] = &[
    ("SessionStart", None, Wait::No),
    ("SessionEnd", None, Wait::No),
    ("UserPromptSubmit", None, Wait::No),
    ("PostToolUse", Some("*"), Wait::No),
    ("Notification", None, Wait::No),
    ("Stop", None, Wait::Stop),
    ("PreToolUse", Some("AskUserQuestion"), Wait::Question),
    ("PermissionRequest", Some("*"), Wait::Approval),
];
const NOTIFY_TIMEOUT_SECS: u64 = 5;
const QUESTION_TIMEOUT_SECS: u64 = 60;
const STOP_TIMEOUT_SECS: u64 = 5;

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

impl ClaudeInstaller {
    /// (event, matcher, command, timeout) for every hook this version installs.
    fn expected(&self, hook: &Path) -> Vec<(&'static str, Option<&'static str>, String, u64)> {
        EVENTS
            .iter()
            .map(|(event, matcher, wait)| {
                let (wait_secs, timeout) = match wait {
                    Wait::No => (None, NOTIFY_TIMEOUT_SECS),
                    Wait::Approval => (Some(self.ctx.timeout_secs), self.ctx.timeout_secs + 10),
                    Wait::Question => (Some(QUESTION_TIMEOUT_SECS), QUESTION_TIMEOUT_SECS + 10),
                    Wait::Stop => (Some(STOP_TIMEOUT_SECS), STOP_TIMEOUT_SECS + 5),
                };
                let command = hook_command(hook, "claude", event, wait_secs.is_some(), wait_secs.unwrap_or(0));
                (*event, *matcher, command, timeout)
            })
            .collect()
    }
}

impl HookInstaller for ClaudeInstaller {
    fn agent(&self) -> &'static str {
        "claude"
    }

    fn status(&self) -> HookStatus {
        let (installed, stale, error) = match read_config(&self.config_path) {
            Ok(cfg) => {
                let cfg = Value::Object(cfg);
                let paths = installed_hook_paths(&cfg);
                let text = cfg.to_string();
                // Stale if the hook binary is gone, or an older Peekaboo installed different hooks.
                let outdated = self
                    .expected(&deployed_hook_path(&self.ctx))
                    .iter()
                    .any(|(_, _, cmd, _)| !text.contains(&Value::String(cmd.clone()).to_string()));
                (!paths.is_empty(), paths.iter().any(|p| !p.exists()) || outdated, None)
            }
            Err(e) => (false, false, Some(e.to_string())),
        };
        HookStatus { agent: "claude".into(), installed, stale, config_path: self.config_path.display().to_string(), error }
    }

    fn install(&self) -> Result<Option<PathBuf>, InstallError> {
        let mut cfg = read_config(&self.config_path)?;
        let hook = deploy_hook(&self.ctx)?;
        let backup = backup(&self.config_path, &self.ctx.backup_dir, "claude")?;
        strip_ours(&mut cfg);
        let hooks = cfg.entry("hooks").or_insert_with(|| json!({}));
        let Value::Object(hooks) = hooks else {
            return Err(InstallError::Malformed("\"hooks\" is not an object".into()));
        };
        for (event, matcher, command, timeout) in self.expected(&hook) {
            let mut group = json!({"hooks": [{ "type": "command", "command": command, "timeout": timeout }]});
            if let Some(m) = matcher {
                group["matcher"] = json!(m);
            }
            match hooks.entry(event.to_string()).or_insert_with(|| json!([])) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn inst(dir: &std::path::Path) -> ClaudeInstaller {
        let src = dir.join("build/peekaboo-hook");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"#!/bin/sh\n").unwrap();
        ClaudeInstaller {
            config_path: dir.join(".claude/settings.json"),
            ctx: InstallCtx { hook_bin: src, bin_dir: dir.join("data/bin"), backup_dir: dir.join("backups"), timeout_secs: 600 },
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

    #[test]
    fn install_points_at_deployed_copy_of_hook() {
        let d = tempfile::tempdir().unwrap();
        let i = inst(d.path());
        i.install().unwrap();
        let deployed = d.path().join("data/bin").join(format!("peekaboo-hook{}", std::env::consts::EXE_SUFFIX));
        assert!(deployed.exists());
        let v = read(&i.config_path);
        let cmd = v["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.starts_with(&format!("\"{}\"", deployed.display())), "{cmd}");
    }

    #[test]
    fn status_flags_missing_hook_binary_as_stale() {
        let d = tempfile::tempdir().unwrap();
        let i = inst(d.path());
        i.install().unwrap();
        assert!(!i.status().stale);
        std::fs::remove_dir_all(d.path().join("data/bin")).unwrap();
        let st = i.status();
        assert!(st.installed && st.stale);
    }

    #[test]
    fn question_hook_has_short_timeout() {
        let d = tempfile::tempdir().unwrap();
        let i = inst(d.path());
        i.install().unwrap();
        let v = read(&i.config_path);
        let q = &v["hooks"]["PreToolUse"][0]["hooks"][0];
        assert!(q["command"].as_str().unwrap().contains("--wait --timeout-secs 60"));
        assert_eq!(q["timeout"], 70);
    }

    #[test]
    fn stop_hook_waits_briefly_so_queued_prompts_can_be_delivered() {
        let d = tempfile::tempdir().unwrap();
        let i = inst(d.path());
        i.install().unwrap();
        let v = read(&i.config_path);
        let stop = &v["hooks"]["Stop"][0]["hooks"][0];
        assert!(stop["command"].as_str().unwrap().ends_with("--event Stop --wait --timeout-secs 5"), "{stop}");
        assert_eq!(stop["timeout"], 10);
    }

    #[test]
    fn older_install_is_flagged_for_repair() {
        let d = tempfile::tempdir().unwrap();
        let i = inst(d.path());
        i.install().unwrap();
        assert!(!i.status().stale);
        // Simulate a previous Peekaboo version that installed Stop without --wait.
        let text = std::fs::read_to_string(&i.config_path).unwrap()
            .replace("--event Stop --wait --timeout-secs 5", "--event Stop");
        std::fs::write(&i.config_path, text).unwrap();
        assert!(i.status().stale);
        i.install().unwrap();
        assert!(!i.status().stale, "repair = reinstall");
    }
}
