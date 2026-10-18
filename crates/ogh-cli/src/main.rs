use clap::Parser;

#[derive(Parser)]
#[command(name = "ogh", version)]
struct Cli {}

fn main() {
    Cli::parse();
}
