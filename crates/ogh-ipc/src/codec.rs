use std::fmt;
use std::io::{self, BufRead, Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

pub const MAX_LINE_BYTES: usize = 1 << 20;

#[derive(Debug)]
pub enum CodecError {
    Io(io::Error),
    TooLong,
    Json(String),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::Io(e) => write!(f, "io error: {e}"),
            CodecError::TooLong => write!(f, "message longer than {MAX_LINE_BYTES} bytes"),
            CodecError::Json(e) => write!(f, "invalid message: {e}"),
        }
    }
}

impl std::error::Error for CodecError {}

impl From<io::Error> for CodecError {
    fn from(e: io::Error) -> Self {
        CodecError::Io(e)
    }
}

pub fn write_line<W: Write, T: Serialize>(w: &mut W, value: &T) -> io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(io::Error::from)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()
}

/// After any `CodecError` the stream may be desynchronised; drop it instead of reading again.
pub fn read_line<R: BufRead, T: DeserializeOwned>(r: &mut R) -> Result<Option<T>, CodecError> {
    let mut buf = Vec::new();
    let read = r
        .by_ref()
        .take(MAX_LINE_BYTES as u64 + 1)
        .read_until(b'\n', &mut buf)?;
    if read == 0 {
        return Ok(None);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
    }
    if buf.len() > MAX_LINE_BYTES {
        return Err(CodecError::TooLong);
    }
    serde_json::from_slice(&buf)
        .map(Some)
        .map_err(|e| CodecError::Json(e.to_string()))
}
