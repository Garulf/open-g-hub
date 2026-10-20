/// Reports an error that stops `program` from starting. A Windows build has no console, so
/// there it also shows the error in a message box.
pub fn report_startup_error(program: &str, error: &str) {
    let message = format!("{program}: {error}");
    eprintln!("{message}");
    #[cfg(windows)]
    message_box(&message);
}

#[cfg(windows)]
fn message_box(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain([0]).collect() };
    let text = wide(message);
    let title = wide("open-g-hub");
    // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call, and a null
    // owner window is allowed.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
