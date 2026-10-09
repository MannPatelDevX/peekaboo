use tauri::{LogicalPosition, LogicalSize, WebviewWindow};

const COLLAPSED: (f64, f64) = (220.0, 44.0);
const EXPANDED: (f64, f64) = (440.0, 560.0);

/// Sizes the overlay and pins it to the top-center of its monitor (over the notch on Macs).
pub fn place(w: &WebviewWindow, expanded: bool) -> tauri::Result<()> {
    let (width, height) = if expanded { EXPANDED } else { COLLAPSED };
    w.set_size(LogicalSize::new(width, height))?;
    if let Some(m) = w.current_monitor()?.or(w.primary_monitor()?) {
        let scale = m.scale_factor();
        let x = m.position().x as f64 / scale + (m.size().width as f64 / scale - width) / 2.0;
        w.set_position(LogicalPosition::new(x, m.position().y as f64 / scale))?;
    }
    Ok(())
}

pub fn setup(w: &WebviewWindow) -> tauri::Result<()> {
    place(w, false)?;
    #[cfg(target_os = "macos")]
    raise_above_menu_bar(w);
    w.set_visible_on_all_workspaces(true)?;
    w.show()
}

/// Normal always-on-top windows sit below the menu bar; the notch lives inside it.
#[cfg(target_os = "macos")]
fn raise_above_menu_bar(w: &WebviewWindow) {
    const NS_STATUS_WINDOW_LEVEL: isize = 25;
    if let Ok(ptr) = w.ns_window() {
        unsafe {
            let win: &objc2::runtime::AnyObject = &*(ptr as *mut objc2::runtime::AnyObject);
            let _: () = objc2::msg_send![win, setLevel: NS_STATUS_WINDOW_LEVEL];
        }
    }
}

#[tauri::command]
pub fn set_expanded(window: WebviewWindow, expanded: bool) -> Result<(), String> {
    place(&window, expanded).map_err(|e| e.to_string())
}
