use serde::Serialize;
use tauri::{LogicalPosition, LogicalSize, WebviewWindow};

/// Width added on each side of the notch for the island's content ("ears").
pub const EAR: f64 = 92.0;
/// Collapsed pill on screens without a notch.
pub const PILL_NO_NOTCH: (f64, f64) = (240.0, 34.0);
pub const EXPANDED: (f64, f64) = (460.0, 560.0);
/// The Sessions board needs room for several columns.
pub const BOARD: (f64, f64) = (820.0, 600.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Size {
    Collapsed,
    Panel,
    Board,
}

/// The camera housing on notched MacBooks, in points. Zero on other screens and platforms.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Notch {
    pub height: f64,
    pub width: f64,
}

impl Notch {
    pub const NONE: Notch = Notch { height: 0.0, width: 0.0 };

    /// From `NSScreen`: the safe-area top inset and the widths of the menu-bar areas left and
    /// right of the camera. The notch is what is left between them.
    pub fn from_areas(screen_width: f64, top_inset: f64, left: f64, right: f64) -> Notch {
        if top_inset <= 0.0 || left <= 0.0 || right <= 0.0 {
            return Notch::NONE;
        }
        Notch { height: top_inset, width: (screen_width - left - right).max(0.0) }
    }

    fn present(&self) -> bool {
        self.height > 0.0
    }
}

pub fn window_size(size: Size, notch: Notch) -> (f64, f64) {
    match size {
        Size::Panel => (EXPANDED.0, EXPANDED.1 + notch.height),
        Size::Board => (BOARD.0, BOARD.1 + notch.height),
        Size::Collapsed if notch.present() => (notch.width + 2.0 * EAR, notch.height),
        Size::Collapsed => PILL_NO_NOTCH,
    }
}

pub fn notch(w: &WebviewWindow) -> Notch {
    #[cfg(target_os = "macos")]
    {
        // Tauri and AppKit name displays differently; match on left edge and width in points.
        let target = crate::screens::monitor_of(w).map(|m| {
            let scale = m.scale_factor();
            (m.position().x as f64 / scale, m.size().width as f64 / scale)
        });
        measure_notch(target)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = w;
        Notch::NONE
    }
}

/// Sizes the overlay and pins it to the top-center of its monitor (over the notch on Macs).
pub fn place(w: &WebviewWindow, size: Size) -> tauri::Result<()> {
    let (width, height) = window_size(size, notch(w));
    w.set_size(LogicalSize::new(width, height))?;
    // Each overlay window owns one display (see screens.rs).
    if let Some(m) = crate::screens::monitor_of(w) {
        let scale = m.scale_factor();
        let x = m.position().x as f64 / scale + (m.size().width as f64 / scale - width) / 2.0;
        w.set_position(LogicalPosition::new(x, m.position().y as f64 / scale))?;
    }
    Ok(())
}

pub fn setup(w: &WebviewWindow) -> tauri::Result<()> {
    place(w, Size::Collapsed)?;
    eprintln!("peekaboo: {} notch {:?}", w.label(), notch(w));
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
mod appkit {
    use objc2::encode::{Encode, Encoding};

    #[repr(C)]
    #[allow(dead_code)] // layout must match AppKit
    pub struct NSEdgeInsets {
        pub top: f64,
        pub left: f64,
        pub bottom: f64,
        pub right: f64,
    }
    unsafe impl Encode for NSEdgeInsets {
        const ENCODING: Encoding = Encoding::Struct("NSEdgeInsets", &[f64::ENCODING; 4]);
    }

    #[repr(C)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }
    unsafe impl Encode for CGPoint {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING; 2]);
    }

    #[repr(C)]
    pub struct CGSize {
        pub width: f64,
        pub height: f64,
    }
    unsafe impl Encode for CGSize {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING; 2]);
    }

    #[repr(C)]
    #[allow(dead_code)]
    pub struct CGRect {
        pub origin: CGPoint,
        pub size: CGSize,
    }
    unsafe impl Encode for CGRect {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
    }
}

/// Reads `safeAreaInsets` and `auxiliaryTopLeftArea`/`auxiliaryTopRightArea` (macOS 12+) of the
/// screen at `display` = (left edge, width) in points (the primary screen when unknown).
#[cfg(target_os = "macos")]
fn measure_notch(display: Option<(f64, f64)>) -> Notch {
    use appkit::{CGRect, NSEdgeInsets};
    use objc2::runtime::{AnyClass, AnyObject};
    unsafe {
        let Some(cls) = AnyClass::get(c"NSScreen") else { return Notch::NONE };
        let screens: *mut AnyObject = objc2::msg_send![cls, screens];
        if screens.is_null() {
            return Notch::NONE;
        }
        let count: usize = objc2::msg_send![&*screens, count];
        let mut chosen: *mut AnyObject = std::ptr::null_mut();
        for i in 0..count {
            let s: *mut AnyObject = objc2::msg_send![&*screens, objectAtIndex: i];
            if s.is_null() {
                continue;
            }
            let matches = match display {
                // screens[0] is the primary display: the default when there is nothing to match.
                None => i == 0,
                Some((x, width)) => {
                    let frame: CGRect = objc2::msg_send![&*s, frame];
                    (frame.origin.x - x).abs() < 1.0 && (frame.size.width - width).abs() < 1.0
                }
            };
            if matches {
                chosen = s;
                break;
            }
        }
        if chosen.is_null() {
            return Notch::NONE;
        }
        let screen = &*chosen;
        let responds = |sel| -> bool { objc2::msg_send![screen, respondsToSelector: sel] };
        if !responds(objc2::sel!(safeAreaInsets)) || !responds(objc2::sel!(auxiliaryTopLeftArea)) {
            return Notch::NONE;
        }
        let insets: NSEdgeInsets = objc2::msg_send![screen, safeAreaInsets];
        let frame: CGRect = objc2::msg_send![screen, frame];
        let left: CGRect = objc2::msg_send![screen, auxiliaryTopLeftArea];
        let right: CGRect = objc2::msg_send![screen, auxiliaryTopRightArea];
        Notch::from_areas(frame.size.width, insets.top, left.size.width, right.size.width)
    }
}

/// Lets the UI lay itself out around the notch.
#[tauri::command]
pub fn get_notch(window: WebviewWindow) -> Notch {
    notch(&window)
}

#[tauri::command]
pub fn set_size(window: WebviewWindow, size: Size) -> Result<(), String> {
    place(&window, size).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MBP: Notch = Notch { height: 38.0, width: 200.0 };

    #[test]
    fn collapsed_island_hugs_the_notch() {
        // Content sits in the "ears" either side of the camera, so the island is exactly notch-tall.
        assert_eq!(window_size(Size::Collapsed, MBP), (200.0 + 2.0 * EAR, 38.0));
    }

    #[test]
    fn no_notch_gets_a_floating_pill() {
        assert_eq!(window_size(Size::Collapsed, Notch::NONE), (PILL_NO_NOTCH.0, PILL_NO_NOTCH.1));
    }

    #[test]
    fn expanded_panel_adds_the_notch_band() {
        assert_eq!(window_size(Size::Panel, MBP), (EXPANDED.0, EXPANDED.1 + 38.0));
        assert_eq!(window_size(Size::Panel, Notch::NONE), EXPANDED);
    }

    #[test]
    fn board_is_wider() {
        assert_eq!(window_size(Size::Board, MBP), (BOARD.0, BOARD.1 + 38.0));
        assert!(BOARD.0 > EXPANDED.0);
    }

    #[test]
    fn notch_from_screen_areas() {
        assert_eq!(Notch::from_areas(1512.0, 38.0, 656.0, 656.0), Notch { height: 38.0, width: 200.0 });
        assert_eq!(Notch::from_areas(1920.0, 0.0, 0.0, 0.0), Notch::NONE);
    }
}
