//! One overlay window per connected display. `main` (from tauri.conf.json) sits on the primary
//! display; every other display gets an `overlay-N` window. Displays are re-checked periodically
//! so plugging a monitor in or out adds or removes its pill.
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Monitor, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const MAIN: &str = "main";
const RESCAN: Duration = Duration::from_secs(3);

/// Which display each overlay window belongs to: window label → display key.
#[derive(Default)]
pub struct Screens(pub Mutex<HashMap<String, String>>);

/// Stable identity of a display: its name plus where it sits.
pub fn monitor_key(m: &Monitor) -> String {
    let p = m.position();
    format!("{}@{},{}", m.name().map(String::as_str).unwrap_or("display"), p.x, p.y)
}

#[derive(Debug, PartialEq)]
pub struct Plan {
    /// Display the `main` window goes on.
    pub main: String,
    /// Displays that need a new window.
    pub create: Vec<String>,
    /// Extra windows whose display is gone (or is now covered by `main`).
    pub close: Vec<String>,
}

/// `extra` maps existing extra-window labels to their display keys.
pub fn plan(extra: &HashMap<String, String>, displays: &[String], primary: &str) -> Plan {
    let wanted: Vec<&String> = displays.iter().filter(|d| d.as_str() != primary).collect();
    let mut close: Vec<String> = extra
        .iter()
        .filter(|(_, key)| !wanted.contains(key))
        .map(|(label, _)| label.clone())
        .collect();
    close.sort();
    let create = wanted.into_iter().filter(|d| !extra.values().any(|k| k == *d)).cloned().collect();
    Plan { main: primary.to_string(), create, close }
}

pub fn next_label(extra: &HashMap<String, String>) -> String {
    (1..).map(|n| format!("overlay-{n}")).find(|l| !extra.contains_key(l)).expect("unbounded")
}

/// The display a window belongs to, falling back to the primary display.
pub fn monitor_of(w: &WebviewWindow) -> Option<Monitor> {
    let key = w.state::<Screens>().0.lock().unwrap().get(w.label()).cloned();
    let monitors = w.available_monitors().ok()?;
    key.and_then(|k| monitors.into_iter().find(|m| monitor_key(m) == k)).or_else(|| w.primary_monitor().ok().flatten())
}

fn build_overlay(app: &AppHandle, label: &str) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title("Peekaboo")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .resizable(false)
        .skip_taskbar(true)
        .shadow(false)
        .focused(false)
        .visible(false)
        .disable_drag_drop_handler()
        .build()
}

/// Brings windows in line with the connected displays. Runs on the main thread.
pub fn sync(app: &AppHandle) {
    let Ok(monitors) = app.available_monitors() else { return };
    let Some(primary) = app.primary_monitor().ok().flatten().or_else(|| monitors.first().cloned()) else { return };
    let displays: Vec<String> = monitors.iter().map(monitor_key).collect();
    let state = app.state::<Screens>();
    let extra: HashMap<String, String> = state.0.lock().unwrap().iter().filter(|(l, _)| l.as_str() != MAIN).map(|(l, k)| (l.clone(), k.clone())).collect();
    let plan = plan(&extra, &displays, &monitor_key(&primary));

    let main_moved = state.0.lock().unwrap().insert(MAIN.to_string(), plan.main.clone()).as_ref() != Some(&plan.main);
    for label in &plan.close {
        state.0.lock().unwrap().remove(label);
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.destroy();
        }
    }
    for key in &plan.create {
        let label = next_label(&state.0.lock().unwrap().clone());
        state.0.lock().unwrap().insert(label.clone(), key.clone());
        match build_overlay(app, &label) {
            Ok(w) => {
                if let Err(e) = crate::overlay::setup(&w) {
                    eprintln!("peekaboo: overlay for {key}: {e}");
                }
            }
            Err(e) => {
                state.0.lock().unwrap().remove(&label);
                eprintln!("peekaboo: could not open overlay for {key}: {e}");
            }
        }
    }
    if main_moved {
        if let Some(w) = app.get_webview_window(MAIN) {
            let _ = crate::overlay::place(&w, crate::overlay::Size::Collapsed);
            let _ = w.emit_to(MAIN, "overlay:display-changed", ());
        }
    }
}

/// Re-checks the displays every few seconds.
pub fn watch(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(RESCAN);
        let h = app.clone();
        let _ = app.run_on_main_thread(move || sync(&h));
    });
}

/// True when the mouse pointer is on this window's display. Requests and the shortcut only open
/// the pill the user is looking at.
#[tauri::command]
pub fn cursor_here(window: WebviewWindow) -> bool {
    let (Ok(cursor), Some(m)) = (window.cursor_position(), monitor_of(&window)) else { return true };
    let (p, s) = (m.position(), m.size());
    cursor.x >= p.x as f64 && cursor.x < (p.x + s.width as i32) as f64 && cursor.y >= p.y as f64 && cursor.y < (p.y + s.height as i32) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(l, k)| (l.to_string(), k.to_string())).collect()
    }

    #[test]
    fn creates_a_window_for_each_extra_display() {
        let plan = plan(&map(&[]), &["Built-in@0,0".into(), "DELL@1512,0".into(), "LG@-1920,0".into()], "Built-in@0,0");
        assert_eq!(plan.main, "Built-in@0,0");
        assert_eq!(plan.create, vec!["DELL@1512,0".to_string(), "LG@-1920,0".to_string()]);
        assert!(plan.close.is_empty());
    }

    #[test]
    fn keeps_existing_windows_and_closes_ones_for_unplugged_displays() {
        let current = map(&[("overlay-1", "DELL@1512,0"), ("overlay-2", "LG@-1920,0")]);
        let plan = plan(&current, &["Built-in@0,0".into(), "DELL@1512,0".into()], "Built-in@0,0");
        assert!(plan.create.is_empty());
        assert_eq!(plan.close, vec!["overlay-2".to_string()]);
    }

    #[test]
    fn primary_display_change_moves_main_and_frees_the_old_one() {
        // The external monitor becomes primary: main moves there, and the built-in screen needs
        // its own window while the external one's old extra window is closed.
        let current = map(&[("overlay-1", "DELL@0,0")]);
        let plan = plan(&current, &["Built-in@-1512,0".into(), "DELL@0,0".into()], "DELL@0,0");
        assert_eq!(plan.main, "DELL@0,0");
        assert_eq!(plan.create, vec!["Built-in@-1512,0".to_string()]);
        assert_eq!(plan.close, vec!["overlay-1".to_string()]);
    }

    #[test]
    fn next_label_skips_taken_ones() {
        assert_eq!(next_label(&map(&[("overlay-1", "a"), ("overlay-3", "b")])), "overlay-2");
        assert_eq!(next_label(&map(&[])), "overlay-1");
    }
}
