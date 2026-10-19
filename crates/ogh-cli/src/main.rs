use std::io;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use clap::Parser;
use ogh_cli::commands::{Cli, Command, RunOptions, run};
use ogh_cli::error::CliError;
use ogh_device::Session;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match execute(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ogh: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}

fn execute(cli: &Cli) -> Result<(), CliError> {
    let options = RunOptions::default();
    if matches!(cli.command, Command::Watch { .. }) {
        stop_on_ctrl_c(&options)?;
    }
    let mut session = Session::open().map_err(CliError::Opening)?;
    run(
        &cli.command,
        &mut session,
        &mut io::stdout().lock(),
        &options,
    )
}

fn stop_on_ctrl_c(options: &RunOptions) -> Result<(), CliError> {
    let stop = Arc::clone(&options.stop);
    ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst))
        .map_err(|e| CliError::Local(format!("cannot install the Ctrl-C handler: {e}")))
}
