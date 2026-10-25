//! The window frame: Windows 11's Mica behind the WinUI style, and a title bar that
//! matches the theme in the other styles.

use slint::Color;

use crate::AppWindow;

/// What the frame should look like.
pub struct Frame {
    pub mica: bool,
    pub dark: bool,
    /// Title bar background and text; ignored with Mica, which paints the title bar itself.
    pub caption: Color,
    pub caption_text: Color,
}

/// Applies the frame and returns whether Mica is showing. Without a native window yet,
/// or on Windows before 11 22H2, there is no Mica and the app paints its own background.
#[cfg(windows)]
pub fn apply(window: &AppWindow, frame: &Frame) -> bool {
    use windows_sys::Win32::Graphics::Dwm::{
        DWMSBT_MAINWINDOW, DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT, DWMWA_SYSTEMBACKDROP_TYPE,
        DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
    };

    /// No system backdrop.
    const DWMSBT_NONE: i32 = 1;

    let Ok(Some(hwnd)) = crate::system::native_hwnd(window) else {
        return false;
    };
    let set = |attribute: i32, value: u32| {
        // SAFETY: `hwnd` is this app's live window, and the value is a 4-byte DWORD
        // as every attribute used here expects.
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute as u32,
                std::ptr::from_ref(&value).cast(),
                size_of::<u32>() as u32,
            )
        }
    };
    set(DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(frame.dark));
    let backdrop = if frame.mica {
        DWMSBT_MAINWINDOW
    } else {
        DWMSBT_NONE
    };
    let mica = set(DWMWA_SYSTEMBACKDROP_TYPE, backdrop as u32) >= 0 && frame.mica;
    if mica {
        set(DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT);
        set(DWMWA_TEXT_COLOR, DWMWA_COLOR_DEFAULT);
    } else {
        set(DWMWA_CAPTION_COLOR, colorref(frame.caption));
        set(DWMWA_TEXT_COLOR, colorref(frame.caption_text));
    }
    mica
}

#[cfg(not(windows))]
pub fn apply(_window: &AppWindow, _frame: &Frame) -> bool {
    false
}

/// `0x00bbggrr`, as the DWM wants colors.
#[cfg(windows)]
fn colorref(c: Color) -> u32 {
    u32::from(c.red()) | u32::from(c.green()) << 8 | u32::from(c.blue()) << 16
}
