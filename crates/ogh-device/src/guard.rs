//! Detects G Hub's agent, which holds the device and must not share it.

use crate::error::DeviceError;

const AGENT_EXE: &str = "lghub_agent.exe";

fn is_ghub_agent(exe_name: &str) -> bool {
    exe_name.eq_ignore_ascii_case(AGENT_EXE)
}

/// True when a process named `lghub_agent.exe` runs. Always false off
/// Windows, where G Hub does not exist. Fails closed: if the process list
/// cannot be read, that is an error, not "G Hub is not running".
///
/// `Session::open` checks this once. A long-running owner of a session must
/// call it again periodically, since G Hub can start later.
pub fn ghub_agent_running() -> Result<bool, DeviceError> {
    Ok(process_names()?.iter().any(|name| is_ghub_agent(name)))
}

/// What Process32FirstW and Process32NextW leave in GetLastError once the
/// list is exhausted. Kept here so the outcome logic is tested off Windows.
#[cfg(any(windows, test))]
const ERROR_NO_MORE_FILES: u32 = 18;

#[cfg(any(windows, test))]
fn process_list_error(code: u32) -> DeviceError {
    DeviceError::Io(format!(
        "cannot list running processes to check for G Hub ({AGENT_EXE}): Windows error {code}"
    ))
}

/// Whether a Process32FirstW or Process32NextW call produced an entry. Only
/// the end of the list stops it quietly, so a list cut short by a failure
/// cannot pass for one without G Hub.
#[cfg(any(windows, test))]
fn entry_outcome(found: bool, last_error: u32) -> Result<bool, DeviceError> {
    match (found, last_error) {
        (true, _) => Ok(true),
        (false, ERROR_NO_MORE_FILES) => Ok(false),
        (false, code) => Err(process_list_error(code)),
    }
}

#[cfg(windows)]
fn process_names() -> Result<Vec<String>, DeviceError> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let mut names = Vec::new();
    // SAFETY: the snapshot handle is checked before use and closed once; the
    // entry is a properly sized PROCESSENTRY32W that outlives every call.
    let outcome = unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(process_list_error(GetLastError()));
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = entry_outcome(Process32FirstW(snapshot, &mut entry) != 0, GetLastError());
        while more == Ok(true) {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            names.push(String::from_utf16_lossy(&entry.szExeFile[..len]));
            more = entry_outcome(Process32NextW(snapshot, &mut entry) != 0, GetLastError());
        }
        CloseHandle(snapshot);
        more
    };
    outcome.map(|_| names)
}

#[cfg(not(windows))]
fn process_names() -> Result<Vec<String>, DeviceError> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_name_matches_in_any_case() {
        assert!(is_ghub_agent("lghub_agent.exe"));
        assert!(is_ghub_agent("LGHUB_Agent.EXE"));
        assert!(!is_ghub_agent("lghub.exe"));
        assert!(!is_ghub_agent("lghub_agent.exe.bak"));
    }

    #[test]
    fn an_entry_found_continues_the_list() {
        assert_eq!(entry_outcome(true, 0), Ok(true));
    }

    #[test]
    fn no_more_files_ends_the_list() {
        assert_eq!(entry_outcome(false, ERROR_NO_MORE_FILES), Ok(false));
    }

    #[test]
    fn any_other_failure_is_an_error_instead_of_an_empty_list() {
        const ERROR_ACCESS_DENIED: u32 = 5;
        let error = entry_outcome(false, ERROR_ACCESS_DENIED).unwrap_err();

        assert!(matches!(error, DeviceError::Io(_)), "{error:?}");
        assert!(error.to_string().contains("Windows error 5"), "{error}");
        assert!(error.to_string().contains(AGENT_EXE), "{error}");
    }

    #[cfg(windows)]
    #[test]
    fn no_more_files_is_the_win32_code() {
        assert_eq!(
            ERROR_NO_MORE_FILES,
            windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES
        );
    }
}
