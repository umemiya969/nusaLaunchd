use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "nusaload")]
#[command(about = "NusaLaunchd control tool", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Connect to NusaLaunchd daemon
    Connect {
        /// Socket path
        #[arg(short = 's', long, default_value = "/run/nusalaunchd/control.sock")]
        socket: PathBuf,
    },
    
    /// List available commands
    Help,
}

fn main() {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Connect { socket } => {
            println!("Connecting to socket: {}", socket.display());
            // TODO: Implement socket connection
            println!("(Control tool implementation coming in Week 3-4)");
        }
        Commands::Help => {
            println!("NusaLaunchd Control Tool (nusaload)");
            println!();
            println!("Available commands:");
            println!("  connect    - Connect to NusaLaunchd daemon");
            println!("  help       - Show this help message");
            println!();
            println!("Note: Full control tool implementation will be completed in Week 3-4");
        }
    }
}