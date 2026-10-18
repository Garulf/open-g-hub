use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use ogh_capture::usbpcap::{Direction, Transfer, read_hid_transfers};
use serde::Serialize;

/// Convert a USBPcap capture into JSONL of its HID interrupt transfers.
#[derive(Parser)]
struct Args {
    /// USBPcap capture file
    input: PathBuf,
    /// Note describing the capture, echoed to stderr so stdout stays pure JSONL
    #[arg(long)]
    label: Option<String>,
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("ogh-capture: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    let bytes = std::fs::read(&args.input)
        .map_err(|e| format!("cannot read {}: {e}", args.input.display()))?;
    let transfers = read_hid_transfers(&bytes).map_err(|e| e.to_string())?;

    if let Some(label) = &args.label {
        eprintln!("ogh-capture: {label} ({} transfers)", transfers.len());
    }
    write_jsonl(&transfers).map_err(|e| format!("cannot write output: {e}"))
}

fn write_jsonl(transfers: &[Transfer]) -> io::Result<()> {
    let mut out = BufWriter::new(io::stdout().lock());
    for transfer in transfers {
        writeln!(out, "{}", jsonl_line(transfer))?;
    }
    out.flush()
}

#[derive(Serialize)]
struct Line {
    t_ms: f64,
    dir: &'static str,
    hex: String,
}

fn jsonl_line(transfer: &Transfer) -> String {
    let dir = match transfer.dir {
        Direction::Out => "out",
        Direction::In => "in",
    };
    let line = Line {
        t_ms: transfer.t_ms,
        dir,
        hex: hex(&transfer.data),
    };
    serde_json::to_string(&line).expect("a plain struct always serializes")
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_line_with_exact_key_order_and_hex() {
        let transfer = Transfer {
            t_ms: 12.5,
            dir: Direction::Out,
            data: vec![0x02, 0x0c, 0xff],
        };
        assert_eq!(
            jsonl_line(&transfer),
            r#"{"t_ms":12.5,"dir":"out","hex":"020cff"}"#
        );
    }
}
