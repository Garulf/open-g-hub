//! Detects G Hub's agent, which holds the device and must not share it.

const AGENT_EXE: &str = "lghub_agent.exe";

fn is_ghub_agent(exe_name: &str) -> bool {
    exe_name.eq_ignore_ascii_case(AGENT_EXE)
}

/// True when a process named `lghub_agent.exe` runs. Always false off
/// Windows, where G Hub does not exist.
pub fn ghub_agent_running() -> bool {
    process_names().iter().any(|name| is_ghub_agent(name))
}

#[cfg(windows)]
fn process_names() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
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
            return names;
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
    names
}

#[cfg(not(windows))]
fn process_names() -> Vec<String> {
    Vec::new()
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
