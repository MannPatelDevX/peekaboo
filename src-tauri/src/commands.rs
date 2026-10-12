use peekaboo_core::claudex::{Action, Claudex, ClaudexSnapshot};
use peekaboo_core::installer::{installers, HookInstaller, InstallCtx, InstallError};
use peekaboo_core::model::{now_secs, AlwaysRule, AppState, Decision, SessionState};
use peekaboo_core::sessions_index::{claude_projects_dir, SessionIndex, TranscriptSession};
use peekaboo_core::terminal::{resume_command, resume_in_terminal, Os};
use peekaboo_core::Core;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;

pub struct AppCtx {
    pub core: Arc<Core>,
    pub install: InstallCtx,
    pub transcripts: Arc<Mutex<SessionIndex>>,
    pub claudex: Arc<Claudex>,
}

/// Runs blocking work (file scans, claudex) off the main thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
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
pub fn remove_rule(ctx: State<AppCtx>, rule: AlwaysRule) -> Result<(), String> {
    ctx.core.remove_rule(&rule)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRow {
    #[serde(flatten)]
    pub session: TranscriptSession,
    /// Live state when Peekaboo's hooks are tracking this session right now.
    pub live: Option<SessionState>,
    pub resume_command: Option<String>,
}

#[tauri::command]
pub async fn list_transcripts(ctx: State<'_, AppCtx>) -> Result<Vec<TranscriptRow>, String> {
    let index = ctx.transcripts.clone();
    let sessions = blocking(move || index.lock().unwrap().scan(&claude_projects_dir(), now_secs())).await?;
    let live = ctx.core.snapshot().sessions;
    let copy_os = if cfg!(windows) { Os::Windows } else { Os::Unix };
    Ok(sessions
        .into_iter()
        .map(|s| {
            let key = format!("claude:{}", s.id);
            TranscriptRow {
                live: live.iter().find(|l| l.key == key).map(|l| l.state),
                resume_command: s.cwd.as_deref().and_then(|c| resume_command(copy_os, c, &s.id).ok()),
                session: s,
            }
        })
        .collect())
}

/// Looks the project folder up from the index rather than trusting a path from the UI.
#[tauri::command]
pub async fn resume_session(ctx: State<'_, AppCtx>, id: String) -> Result<(), String> {
    let index = ctx.transcripts.clone();
    blocking(move || {
        let sessions = index.lock().unwrap().scan(&claude_projects_dir(), now_secs());
        let s = sessions.into_iter().find(|s| s.id == id).ok_or("session not found")?;
        let cwd = s.cwd.ok_or("this session has no project folder recorded")?;
        resume_in_terminal(&cwd, &s.id)
    })
    .await?
}

#[tauri::command]
pub async fn claudex_snapshot(ctx: State<'_, AppCtx>, force: bool) -> Result<ClaudexSnapshot, String> {
    let c = ctx.claudex.clone();
    blocking(move || c.snapshot(force)).await
}

#[tauri::command]
pub async fn claudex_act(ctx: State<'_, AppCtx>, action: Action) -> Result<String, String> {
    let c = ctx.claudex.clone();
    blocking(move || c.act(&action)).await?
}
