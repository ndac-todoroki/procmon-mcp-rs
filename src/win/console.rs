//! Console mode for colored output.

use windows::Win32::System::Console::{
    CONSOLE_MODE, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle,
    STD_ERROR_HANDLE, SetConsoleMode,
};

/// Turns on escape sequence processing for stderr. False when stderr is not a console
/// or the console cannot interpret escape sequences.
pub fn enable_ansi_on_stderr() -> bool {
    // SAFETY: the standard handle is owned by the process and not closed here.
    unsafe {
        let Ok(handle) = GetStdHandle(STD_ERROR_HANDLE) else {
            return false;
        };
        let mut mode = CONSOLE_MODE::default();
        if GetConsoleMode(handle, &mut mode).is_err() {
            return false;
        }
        mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING == ENABLE_VIRTUAL_TERMINAL_PROCESSING
            || SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING).is_ok()
    }
}
