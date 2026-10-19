use std::ffi::c_void;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_ANY, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};

use crate::error::CliError;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "open-g-hub";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn failure(action: &str, code: WIN32_ERROR) -> CliError {
    CliError::Local(format!(
        "cannot {action} the auto-start entry (Win32 error {code})"
    ))
}

/// Writes the Run value as a NUL-terminated REG_SZ.
pub fn set(command: &str) -> Result<(), CliError> {
    let key = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    let data = wide(command);
    let byte_len = u32::try_from(data.len() * size_of::<u16>())
        .map_err(|_| CliError::Local("the daemon path is too long".to_string()))?;
    // SAFETY: `key`, `name` and `data` are NUL-terminated UTF-16 buffers that outlive
    // the call, and `byte_len` is the exact size of `data` including its NUL.
    let code = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            data.as_ptr().cast::<c_void>(),
            byte_len,
        )
    };
    match code {
        ERROR_SUCCESS => Ok(()),
        code => Err(failure("write", code)),
    }
}

/// Deletes the Run value; a value that is already gone counts as deleted.
pub fn remove() -> Result<(), CliError> {
    let key = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    // SAFETY: `key` and `name` are NUL-terminated UTF-16 buffers that outlive the call.
    let code = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
    match code {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        code => Err(failure("delete", code)),
    }
}

/// Whether the Run value exists, whatever its type.
pub fn is_set() -> Result<bool, CliError> {
    let key = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    // SAFETY: `key` and `name` are NUL-terminated UTF-16 buffers that outlive the call;
    // the null type, data and size pointers ask only whether the value exists.
    let code = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_ANY,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    match code {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND => Ok(false),
        code => Err(failure("read", code)),
    }
}
