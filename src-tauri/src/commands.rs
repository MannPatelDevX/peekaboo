use peekaboo_core::claudex::{Action, Claudex, ClaudexSnapshot};
use peekaboo_core::groups::{GroupData, GroupStore};
use peekaboo_core::installer::{installers, HookInstaller, InstallCtx, InstallError};
use peekaboo_core::model::{now_secs, AlwaysRule, AppState, Decision, SessionState};
use peekaboo_core::sessions_index::{claude_projects_dir, SessionIndex, TranscriptSession};
use peekaboo_core::terminal::{resume_command, resume_in_terminal, type_into_session, Os};
use peekaboo_core::Core;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;

pub struct AppCtx {
    pub core: Arc<Core>,
    pub install: InstallCtx,
    pub transcripts: Arc<Mutex<SessionIndex>>,
    pub claudex: Arc<Claudex>,
    pub groups: Mutex<GroupStore>,
}

/// Runs blocking work (file scans, claudex) off the main thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_state(ctx: State<AppCtx>) -> AppState {
    let mut s = ctx.core.snapshot();
    s.hooks = installers(&ctx.install).iter().map(|i| i.status()).collect();
    // Runs on the main thread: never wait behind a board scan; titles fill in on the next refresh.
    if let Ok(mut index) = ctx.transcripts.try_lock() {
        let root = claude_projects_dir();
        for session in &mut s.sessions {
            if let Some(id) = session.key.strip_prefix("claude:") {
                session.title = index.title_for(&root, id);
            }
        }
    }
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

/// Applies `f` to the group store and saves; returns the new state for the UI.
fn with_groups(ctx: &AppCtx, f: impl FnOnce(&mut GroupStore) -> Result<(), String>) -> Result<GroupData, String> {
    let mut store = ctx.groups.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut store)?;
    store.save().map_err(|e| e.to_string())?;
    Ok(store.data().clone())
}

#[tauri::command]
pub fn get_groups(ctx: State<AppCtx>) -> GroupData {
    ctx.groups.lock().unwrap_or_else(|e| e.into_inner()).data().clone()
}

#[tauri::command]
pub fn create_group(ctx: State<AppCtx>, name: String) -> Result<GroupData, String> {
    with_groups(&ctx, |s| s.create(&name).map(|_| ()))
}

#[tauri::command]
pub fn rename_group(ctx: State<AppCtx>, id: String, name: String) -> Result<GroupData, String> {
    with_groups(&ctx, |s| s.rename(&id, &name))
}

#[tauri::command]
pub fn delete_group(ctx: State<AppCtx>, id: String) -> Result<GroupData, String> {
    with_groups(&ctx, |s| s.delete(&id))
}

#[tauri::command]
pub fn assign_session(ctx: State<AppCtx>, session: String, group: Option<String>) -> Result<GroupData, String> {
    with_groups(&ctx, |s| s.assign(&session, group.as_deref()))
}

/// Groups every ungrouped session from the last 30 days by its project folder.
#[tauri::command]
pub async fn auto_group(ctx: State<'_, AppCtx>) -> Result<GroupData, String> {
    let index = ctx.transcripts.clone();
    let sessions = blocking(move || index.lock().unwrap().scan(&claude_projects_dir(), now_secs())).await?;
    let pairs: Vec<(&str, &str)> = sessions.iter().map(|s| (s.id.as_str(), s.project.as_str())).collect();
    with_groups(&ctx, |s| {
        s.auto_group(&pairs);
        Ok(())
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PromptRoute {
    /// Typed into the session's Terminal.app / iTerm2 tab.
    Terminal,
    /// Queued; delivered when the agent next finishes a turn.
    Queued,
}

#[tauri::command]
pub async fn send_prompt(ctx: State<'_, AppCtx>, key: String, text: String) -> Result<PromptRoute, String> {
    if text.trim().is_empty() {
        return Err("message is empty".into());
    }
    let session = ctx.core.snapshot().sessions.into_iter().find(|s| s.key == key);
    let typable = session.as_ref().is_none_or(|s| peekaboo_core::model::can_type_into(s.terminal.as_deref()));
    if let Some(tty) = ctx.core.session_tty(&key).filter(|_| typable) {
        let t = text.clone();
        if blocking(move || type_into_session(&tty, &t)).await?? {
            return Ok(PromptRoute::Terminal);
        }
    }
    ctx.core.queue_prompt(&key, &text)?;
    Ok(PromptRoute::Queued)
}
