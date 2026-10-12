mod commands;
mod overlay;

use commands::AppCtx;
use peekaboo_core::installer::{deploy_hook, installers, InstallCtx};
use peekaboo_core::{ipc, Core};
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// The hook binary ships next to the app executable (sidecar in bundles, same target dir in dev).
fn hook_bin_path() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.with_file_name(format!("peekaboo-hook{}", std::env::consts::EXE_SUFFIX))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let data_dir = app.path().app_data_dir()?;
            let handle = app.handle().clone();
            let core = Arc::new(Core::new(
                data_dir.join("settings.json"),
                Arc::new(move || {
                    let _ = handle.emit("state:changed", ());
                }),
            ));
            let install = InstallCtx {
                hook_bin: hook_bin_path(),
                bin_dir: data_dir.join("bin"),
                backup_dir: data_dir.join("backups"),
                timeout_secs: core.approval_timeout_secs(),
            };
            // Keep the stable hook copy current (app updates, moved bundles) for already-installed agents.
            if installers(&install).iter().any(|i| i.status().installed) {
                if let Err(e) = deploy_hook(&install) {
                    eprintln!("peekaboo: could not refresh hook binary: {e}");
                }
            }
            // Another app may own the shortcut; Peekaboo still works without it.
            if let Err(e) = app.global_shortcut().on_shortcut("CommandOrControl+Alt+P", |app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    let _ = app.emit("overlay:toggle", ());
                }
            }) {
                eprintln!("peekaboo: global shortcut unavailable: {e}");
            }
            app.manage(AppCtx {
                core: core.clone(),
                install,
                transcripts: Default::default(),
                claudex: Arc::new(peekaboo_core::claudex::Claudex::new(peekaboo_core::claudex::find_claudex())),
            });
            tauri::async_runtime::spawn(async move {
                if let Err(e) = ipc::serve(core).await {
                    eprintln!("peekaboo: IPC server stopped: {e}");
                }
            });
            overlay::setup(&app.get_webview_window("main").expect("main window"))?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::decide,
            commands::install_hooks,
            commands::uninstall_hooks,
            commands::remove_rule,
            commands::list_transcripts,
            commands::resume_session,
            commands::claudex_snapshot,
            commands::claudex_act,
            overlay::set_expanded,
            overlay::get_top_inset,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Peekaboo");
}
