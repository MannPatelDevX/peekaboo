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
        let Value::Object(hooks) = hooks else {
            return Err(InstallError::Malformed("\"hooks\" is not an object".into()));
        };
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
