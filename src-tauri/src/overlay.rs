use tauri::{LogicalPosition, LogicalSize, WebviewWindow};

const COLLAPSED: (f64, f64) = (220.0, 44.0);
const EXPANDED: (f64, f64) = (440.0, 560.0);

/// Window size for a given state. `top_inset` is the height hidden by the notch: the window grows
/// by that much and the UI pads its content down, so nothing is drawn behind the camera housing.
pub fn window_size(expanded: bool, top_inset: f64) -> (f64, f64) {
    let (width, height) = if expanded { EXPANDED } else { COLLAPSED };
    (width, height + top_inset)
}

/// Height of the notch on the window's screen (0 on screens without one, and off macOS).
pub fn top_inset(w: &WebviewWindow) -> f64 {
    #[cfg(target_os = "macos")]
    {
        notch_inset(w)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = w;
        0.0
    }
}

/// Sizes the overlay and pins it to the top-center of its monitor (over the notch on Macs).
pub fn place(w: &WebviewWindow, expanded: bool) -> tauri::Result<()> {
    let (width, height) = window_size(expanded, top_inset(w));
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
    eprintln!("peekaboo: overlay top inset {}pt", top_inset(w));
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

#[cfg(target_os = "macos")]
#[repr(C)]
#[allow(dead_code)] // layout must match AppKit; only `top` is read
struct NSEdgeInsets {
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
}

#[cfg(target_os = "macos")]
unsafe impl objc2::encode::Encode for NSEdgeInsets {
    const ENCODING: objc2::encode::Encoding = objc2::encode::Encoding::Struct(
        "NSEdgeInsets",
        &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING],
    );
}

/// `NSScreen.safeAreaInsets.top` (macOS 12+) for the window's screen, falling back to the main screen.
#[cfg(target_os = "macos")]
fn notch_inset(w: &WebviewWindow) -> f64 {
    use objc2::runtime::{AnyClass, AnyObject};
    let Ok(ptr) = w.ns_window() else { return 0.0 };
    unsafe {
        let win = &*(ptr as *mut AnyObject);
        let mut screen: *mut AnyObject = objc2::msg_send![win, screen];
        if screen.is_null() {
            let Some(cls) = AnyClass::get(c"NSScreen") else { return 0.0 };
            screen = objc2::msg_send![cls, mainScreen];
        }
        if screen.is_null() {
            return 0.0;
        }
        let screen = &*screen;
        let responds: bool = objc2::msg_send![screen, respondsToSelector: objc2::sel!(safeAreaInsets)];
        if !responds {
            return 0.0;
        }
        let insets: NSEdgeInsets = objc2::msg_send![screen, safeAreaInsets];
        insets.top.max(0.0)
    }
}

/// Lets the UI pad its content below the notch.
#[tauri::command]
pub fn get_top_inset(window: WebviewWindow) -> f64 {
    top_inset(&window)
}

#[tauri::command]
pub fn set_expanded(window: WebviewWindow, expanded: bool) -> Result<(), String> {
    place(&window, expanded).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_sits_below_the_notch() {
        assert_eq!(window_size(false, 0.0), (220.0, 44.0));
        assert_eq!(window_size(false, 32.0), (220.0, 76.0));
        assert_eq!(window_size(true, 32.0), (440.0, 592.0));
    }
}
