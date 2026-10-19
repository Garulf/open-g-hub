use std::path::{Path, PathBuf};

use directories::ProjectDirs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_file: PathBuf,
    pub state_file: PathBuf,
    pub log_file: PathBuf,
}

impl Paths {
    pub fn for_current_user() -> Result<Paths, String> {
        let dirs = ProjectDirs::from("", "", "open-g-hub")
            .ok_or_else(|| "cannot determine the user's home directory".to_string())?;
        Ok(Paths {
            config_file: dirs.config_dir().join("config.toml"),
            state_file: dirs.data_local_dir().join("state.toml"),
            log_file: dirs.data_local_dir().join("daemon.log"),
        })
    }

    pub fn in_dir(dir: &Path) -> Paths {
        Paths {
            config_file: dir.join("config.toml"),
            state_file: dir.join("state.toml"),
            log_file: dir.join("daemon.log"),
        }
    }
}
