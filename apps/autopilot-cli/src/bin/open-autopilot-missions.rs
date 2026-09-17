use clap::Parser;

#[path = "../mission_cli.rs"]
mod mission_cli;

#[derive(Parser)]
#[command(name = "open-autopilot-missions", version, about = "OPEN AUTOPILOT durable mission queue")]
struct MissionCli {
    #[command(subcommand)]
    command: mission_cli::MissionCommands,
}

#[tokio::main]
async fn main() -> Result<(), String> {
    mission_cli::handle(MissionCli::parse().command).await
}
