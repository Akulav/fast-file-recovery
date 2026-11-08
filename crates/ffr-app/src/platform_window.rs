//! Small per-OS window helpers: sharp taskbar/Alt-Tab icons on Windows and attaching to the
//! parent console for `--version` in release (GUI subsystem) builds.

#[cfg(windows)]
mod imp {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, WM_SETICON,
    };

    /// Send WM_SETICON with the 16 px and 32 px frames from the exe's icon resource (id 1).
    pub fn set_window_icons(cc: &eframe::CreationContext<'_>) {
        let Ok(handle) = cc.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(w) = handle.as_raw() else {
            return;
        };
        let hwnd = w.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        // SAFETY: a null module name returns this executable's handle.
        let module = unsafe { GetModuleHandleW(std::ptr::null()) };
        for (kind, size) in [(ICON_SMALL, 16), (ICON_BIG, 32)] {
            // SAFETY: resource id 1 is passed as MAKEINTRESOURCE; the module handle is valid.
            let icon =
                unsafe { LoadImageW(module, 1 as _, IMAGE_ICON, size, size, LR_DEFAULTCOLOR) };
            if !icon.is_null() {
                // SAFETY: hwnd is our live window; WM_SETICON takes the icon handle as LPARAM.
                unsafe { SendMessageW(hwnd, WM_SETICON, kind as usize, icon as isize) };
            }
        }
    }

    pub fn attach_console() {
        // SAFETY: no preconditions; failing (no parent console) is harmless.
        unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn set_window_icons(_cc: &eframe::CreationContext<'_>) {}
    pub fn attach_console() {}
}

pub use imp::{attach_console, set_window_icons};
