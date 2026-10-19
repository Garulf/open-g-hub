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
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(DeviceError::Io(format!(
                "cannot list running processes to check for G Hub ({AGENT_EXE}): \
                 Windows error {}",
                GetLastError()
            )));
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            names.push(String::from_utf16_lossy(&entry.szExeFile[..len]));
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    Ok(names)
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
}
