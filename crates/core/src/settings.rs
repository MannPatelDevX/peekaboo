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
