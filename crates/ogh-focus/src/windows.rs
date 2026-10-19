use std::cell::RefCell;
use std::path::Path;
use std::ptr;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Sender, bounded};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, MSG,
    PostQuitMessage, WINEVENT_OUTOFCONTEXT,
};

use crate::Unavailable;

const MAX_PATH_UTF16: usize = 32 * 1024;

thread_local! {
    static ON_FOCUS: RefCell<Option<Sender<Option<String>>>> = const { RefCell::new(None) };
}

pub(crate) fn spawn(on_focus: Sender<Option<String>>) -> Result<JoinHandle<()>, Unavailable> {
    let (started_tx, started) = bounded::<Result<(), String>>(1);
    let handle = thread::spawn(move || run(on_focus, started_tx));
    match started.recv() {
        Ok(Ok(())) => Ok(handle),
        Ok(Err(reason)) => Err(Unavailable(reason)),
        Err(_) => Err(Unavailable(
            "the focus watcher thread stopped while starting".to_string(),
        )),
    }
}

fn run(on_focus: Sender<Option<String>>, started: Sender<Result<(), String>>) {
    // SAFETY: the callback only reads thread-local state and runs on this
    // thread, from inside `GetMessageW`, because of WINEVENT_OUTOFCONTEXT.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            ptr::null_mut(),
            Some(on_foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if hook.is_null() {
        let _ = started.send(Err("cannot install the foreground window hook".to_string()));
        return;
    }
    // SAFETY: no pointers are involved.
    let current = app_of(unsafe { GetForegroundWindow() });
    if on_focus.send(current).is_ok() {
        ON_FOCUS.with(|slot| *slot.borrow_mut() = Some(on_focus));
        let _ = started.send(Ok(()));
        pump_messages();
    } else {
        let _ = started.send(Ok(()));
    }
    // SAFETY: `hook` came from `SetWinEventHook` on this thread and is unhooked once.
    unsafe { UnhookWinEvent(hook) };
}

fn pump_messages() {
    // SAFETY: an all-zero MSG is a valid value for the out-parameter.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    // SAFETY: `message` is valid for writes; a null window receives every thread message.
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {}
}

unsafe extern "system" fn on_foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    window: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let app = app_of(window);
    let delivered = ON_FOCUS.with(|slot| match slot.borrow().as_ref() {
        Some(sender) => sender.send(app).is_ok(),
        None => false,
    });
    if !delivered {
        // SAFETY: no pointers are involved; ends this thread's `GetMessageW` loop.
        unsafe { PostQuitMessage(0) };
    }
}

fn app_of(window: HWND) -> Option<String> {
    if window.is_null() {
        return None;
    }
    let mut process_id = 0u32;
    // SAFETY: `process_id` is valid for writes.
    unsafe { GetWindowThreadProcessId(window, &mut process_id) };
    if process_id == 0 {
        return None;
    }
    // SAFETY: no pointers are involved.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return None;
    }
    let path = image_path(process);
    // SAFETY: `process` was opened above and is closed once.
    unsafe { CloseHandle(process) };
    executable_name(&path?)
}

fn image_path(process: HANDLE) -> Option<String> {
    let mut buffer = vec![0u16; MAX_PATH_UTF16];
    let mut length = u32::try_from(buffer.len()).ok()?;
    // SAFETY: `buffer` holds `length` UTF-16 units and `length` is updated to the units written.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &mut length,
        )
    };
    if ok == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

fn executable_name(path: &str) -> Option<String> {
    let name = Path::new(path)
        .file_name()?
        .to_string_lossy()
        .to_lowercase();
    (!name.is_empty()).then_some(name)
}
