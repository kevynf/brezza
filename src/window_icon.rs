use std::sync::Arc;

use egui::IconData;

pub fn mark(dark: bool) -> Arc<IconData> {
    let rgba: &[u8] = if dark {
        include_bytes!(concat!(env!("OUT_DIR"), "/mark-white.rgba"))
    } else {
        include_bytes!(concat!(env!("OUT_DIR"), "/mark-black.rgba"))
    };
    let side = ((rgba.len() / 4) as f64).sqrt() as u32;
    Arc::new(IconData {
        rgba: rgba.to_vec(),
        width: side,
        height: side,
    })
}

#[cfg(windows)]
pub fn use_app_icon_for_taskbar(window: &impl raw_window_handle::HasWindowHandle) {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, ICON_BIG, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CYICON,
        SendMessageW, WM_SETICON,
    };

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    unsafe {
        let icon = LoadImageW(
            GetModuleHandleW(std::ptr::null()),
            1 as _,
            IMAGE_ICON,
            GetSystemMetrics(SM_CXICON),
            GetSystemMetrics(SM_CYICON),
            LR_DEFAULTCOLOR,
        );
        if icon != 0 {
            SendMessageW(handle.hwnd.get(), WM_SETICON, ICON_BIG as usize, icon);
        }
    }
}

#[cfg(not(windows))]
pub fn use_app_icon_for_taskbar(_window: &impl raw_window_handle::HasWindowHandle) {}
