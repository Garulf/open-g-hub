use std::path::{Path, PathBuf};
use std::process::{Child, Command};

const GUI_NAME: &str = if cfg!(windows) {
    "ogh-gui.exe"
} else {
    "ogh-gui"
};

/// The window binary next to the running executable, else its bare name for a `PATH` lookup.
pub fn gui_path(current_exe: &Path) -> PathBuf {
    let sibling = current_exe.with_file_name(GUI_NAME);
    if sibling.is_file() {
        sibling
    } else {
        PathBuf::from(GUI_NAME)
    }
}

pub struct Launcher {
    program: PathBuf,
    child: Option<Child>,
}

impl Launcher {
    pub fn new(program: PathBuf) -> Launcher {
        Launcher {
            program,
            child: None,
        }
    }

    pub fn for_current_exe() -> Result<Launcher, String> {
        let exe = std::env::current_exe()
            .map_err(|error| format!("could not locate the running executable: {error}"))?;
        Ok(Launcher::new(gui_path(&exe)))
    }

    /// Starts the window unless the one started last is still running.
    pub fn open(&mut self) -> Result<(), String> {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(None) => return Ok(()),
                Ok(Some(_)) | Err(_) => self.child = None,
            }
        }
        let child = Command::new(&self.program)
            .spawn()
            .map_err(|error| format!("could not start {}: {error}", self.program.display()))?;
        self.child = Some(child);
        Ok(())
    }
}
