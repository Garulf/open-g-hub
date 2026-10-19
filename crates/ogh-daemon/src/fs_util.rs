use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

/// Writes `contents` to a temporary file beside `path`, then renames it over
/// `path`, creating the parent directory first.
pub(crate) fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)?;
    }
    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_os_string();
    tmp_name.push(OsString::from(".tmp"));
    let tmp = path.with_file_name(tmp_name);
    let mut file = File::create(&tmp)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    #[cfg(unix)]
    File::open(parent.unwrap_or(Path::new(".")))?.sync_all()?;
    Ok(())
}
