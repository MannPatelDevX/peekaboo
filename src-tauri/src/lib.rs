mod commands;
mod overlay;

use commands::AppCtx;
use peekaboo_core::installer::InstallCtx;
use peekaboo_core::{ipc, Core};
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::ShortcutState;

/// The hook binary ships next to the app executable (sidecar in bundles, same target dir in dev).
fn hook_bin_path() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.with_file_name(format!("peekaboo-hook{}", std::env::consts::EXE_SUFFIX))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts(["CommandOrControl+Alt+P"])
                .expect("valid shortcut")
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let _ = app.emit("overlay:toggle", ());
                    }
                })
                .build(),
        )
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
                backup_dir: data_dir.join("backups"),
                timeout_secs: core.approval_timeout_secs(),
            };
            app.manage(AppCtx { core: core.clone(), install });
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
            overlay::set_expanded,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Peekaboo");
}
