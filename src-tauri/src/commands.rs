use peekaboo_core::installer::{installers, HookInstaller, InstallCtx, InstallError};
use peekaboo_core::model::{AlwaysRule, AppState, Decision};
use peekaboo_core::Core;
use std::sync::Arc;
use tauri::State;

pub struct AppCtx {
    pub core: Arc<Core>,
    pub install: InstallCtx,
}

#[tauri::command]
pub fn get_state(ctx: State<AppCtx>) -> AppState {
    let mut s = ctx.core.snapshot();
    s.hooks = installers(&ctx.install).iter().map(|i| i.status()).collect();
    s
}

#[tauri::command]
pub fn decide(ctx: State<AppCtx>, id: String, decision: Decision) -> Result<(), String> {
    ctx.core.decide(&id, decision)
}

fn with_installer<T>(
    ctx: &AppCtx,
    agent: &str,
    f: impl FnOnce(&dyn HookInstaller) -> Result<T, InstallError>,
) -> Result<T, String> {
    let all = installers(&ctx.install);
    let i = all.iter().find(|i| i.agent() == agent).ok_or_else(|| format!("unknown agent {agent}"))?;
    f(i.as_ref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn install_hooks(ctx: State<AppCtx>, agent: String) -> Result<(), String> {
    with_installer(&ctx, &agent, |i| i.install().map(|_| ()))
}

#[tauri::command]
pub fn uninstall_hooks(ctx: State<AppCtx>, agent: String) -> Result<(), String> {
    with_installer(&ctx, &agent, |i| i.uninstall())
}

#[tauri::command]
pub fn remove_rule(ctx: State<AppCtx>, agent: String, tool: String) -> Result<(), String> {
    ctx.core.remove_rule(&AlwaysRule { agent, tool })
}
