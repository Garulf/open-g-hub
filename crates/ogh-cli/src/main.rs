use std::io;
use std::process::ExitCode;

use clap::Parser;
use ogh_cli::commands::{Cli, run};
use ogh_cli::daemon::DaemonConnection;
use ogh_cli::error::CliError;
use ogh_ipc::Endpoint;

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
    let endpoint = Endpoint::for_current_user().map_err(CliError::Endpoint)?;
    let mut daemon = DaemonConnection::new(endpoint);
    run(cli, &mut daemon, &mut io::stdout().lock())
}
