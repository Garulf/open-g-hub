//! `ogh autostart`: start `ogh-daemon` at logon through the per-user Run key on Windows.

use std::io::Write;
use std::path::Path;

use clap::Subcommand;

use crate::error::CliError;

#[cfg(not(windows))]
const NOT_WINDOWS: &str = "use the systemd user unit (or the NixOS/Home Manager module)";

#[cfg(windows)]
mod registry;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Subcommand)]
pub enum AutostartCommand {
    /// Start ogh-daemon.exe, found next to ogh.exe, when you log on
    On,
    /// Stop starting ogh-daemon at logon
    Off,
    /// Print on or off
    Status,
}

/// The Run key value for a daemon at `daemon_exe`: the path in double quotes, so a
/// folder with spaces is not split into a program and arguments.
pub fn autostart_command(daemon_exe: &Path) -> String {
    format!("\"{}\"", daemon_exe.display())
}

#[cfg(not(windows))]
pub fn run(_command: AutostartCommand, out: &mut impl Write) -> Result<(), CliError> {
    writeln!(out, "{NOT_WINDOWS}")?;
    out.flush()?;
    Ok(())
}

#[cfg(windows)]
pub fn run(command: AutostartCommand, out: &mut impl Write) -> Result<(), CliError> {
    let text = match command {
        AutostartCommand::On => {
            let daemon_exe = daemon_exe()?;
            if !daemon_exe.is_file() {
                return Err(CliError::Local(format!(
                    "{} does not exist; put ogh-daemon.exe next to ogh.exe",
                    daemon_exe.display()
                )));
            }
            registry::set(&autostart_command(&daemon_exe))?;
            "auto-start on"
        }
        AutostartCommand::Off => {
            registry::remove()?;
            "auto-start off"
        }
        AutostartCommand::Status => {
            if registry::is_set()? {
                "on"
            } else {
                "off"
            }
        }
    };
    writeln!(out, "{text}")?;
    out.flush()?;
    Ok(())
}

#[cfg(windows)]
fn daemon_exe() -> Result<std::path::PathBuf, CliError> {
    let own = std::env::current_exe()
        .map_err(|e| CliError::Local(format!("cannot find ogh.exe's own path: {e}")))?;
    Ok(own.with_file_name("ogh-daemon.exe"))
}
