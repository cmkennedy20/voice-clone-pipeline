//! Tier 6 containerized CLI entrypoint (`doctor`, `enroll`, `translate-clip`, `bench`).

mod commands;

use std::process::ExitCode;

use clap::Parser;
use framework::{config::AppConfig, telemetry::init_telemetry};

async fn run(cli: &commands::Cli) -> Result<(), commands::CliError> {
    let config = AppConfig::from_env()?;
    init_telemetry(&config.rust_log);
    commands::execute(cli, &config).await
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = commands::Cli::parse();
    if let Err(err) = run(&cli).await {
        init_telemetry("info");
        tracing::error!(error = %err, "CLI command failed");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
