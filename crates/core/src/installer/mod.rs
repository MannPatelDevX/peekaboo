use crate::model::HookStatus;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub mod claude;

/// Every command we install contains this; uninstall removes exactly those entries.
pub const HOOK_MARKER: &str = "peekaboo-hook";

#[derive(Debug, Clone)]
pub struct InstallCtx {
    pub hook_bin: PathBuf,
    pub backup_dir: PathBuf,
    pub timeout_secs: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("config is not a valid JSON object: {0}")]
    Malformed(String),
}

pub trait HookInstaller: Send + Sync {
    fn agent(&self) -> &'static str;
    fn status(&self) -> HookStatus;
    /// Returns the backup path if an existing config was backed up.
    fn install(&self) -> Result<Option<PathBuf>, InstallError>;
    fn uninstall(&self) -> Result<(), InstallError>;
}

pub fn installers(ctx: &InstallCtx) -> Vec<Box<dyn HookInstaller>> {
    vec![Box::new(claude::ClaudeInstaller::new(ctx.clone()))]
}

pub(crate) fn hook_command(bin: &Path, agent: &str, event: &str, wait: bool, timeout_secs: u64) -> String {
    let mut cmd = format!("\"{}\" --agent {agent} --event {event}", bin.display());
    if wait {
        cmd.push_str(&format!(" --wait --timeout-secs {timeout_secs}"));
    }
    cmd
}

/// Reads a JSON-object config. Missing file → empty object.
pub(crate) fn read_config(path: &Path) -> Result<Map<String, Value>, InstallError> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(m)) => Ok(m),
            Ok(_) => Err(InstallError::Malformed(path.display().to_string())),
            Err(e) => Err(InstallError::Malformed(format!("{}: {e}", path.display()))),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn write_config(path: &Path, cfg: &Map<String, Value>) -> Result<(), InstallError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(cfg).map_err(|e| InstallError::Malformed(e.to_string()))?;
    std::fs::write(path, text)?;
    Ok(())
}

pub(crate) fn backup(path: &Path, backup_dir: &Path, agent: &str) -> Result<Option<PathBuf>, InstallError> {
    if !path.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(backup_dir)?;
    let dest = backup_dir.join(format!("{agent}-settings-{}.json", crate::model::now_secs()));
    std::fs::copy(path, &dest)?;
    Ok(Some(dest))
}
