use std::path::PathBuf;
use clap::Parser;
use tracing::{info, error, warn};
use tracing_subscriber;

mod job;
mod process;
mod event;
mod cli;
mod util;

use job::JobManager;
use util::error::Result;
use cli::{CliArgs, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = CliArgs::parse();
    
    // Initialize logging
    init_logging(&args.log_level.to_string());
    
    info!("Starting NusaLaunchd v{}", env!("CARGO_PKG_VERSION"));
    
    match args.command {
        Some(Commands::Daemon { daemon_opts }) => {
            run_daemon(&args, daemon_opts).await
        }
        Some(Commands::Job { job_command }) => {
            handle_job_command(job_command, &args).await
        }
        Some(Commands::Validate { path, strict }) => {
            validate_config(path, strict).await
        }
        Some(Commands::Status { detailed, watch, format }) => {
            show_status(detailed, watch, format).await
        }
        Some(Commands::Example { example_type, output }) => {
            generate_example(example_type, output).await
        }
        Some(Commands::Socket { socket_command }) => {
            handle_socket_command(socket_command).await
        }
        None => {
            // Default command: run as daemon
            info!("No command specified, running as daemon");
            run_daemon(&args, cli::args::DaemonOptions::default()).await
        }
    }
}

async fn run_daemon(args: &CliArgs, _daemon_opts: cli::args::DaemonOptions) -> Result<()> {
    info!("Starting NusaLaunchd daemon");
    
    // Create job manager
    let (job_manager, event_rx) = JobManager::new().await?;
    
    // Start event processor
    let event_handle = tokio::spawn(event::EventDispatcher::process_events(event_rx));
    
    // Load jobs from config directory
    load_jobs_from_directory(&job_manager, &args.config_dir).await?;
    
    if args.dry_run {
        info!("Dry run mode - not starting jobs");
        show_daemon_status(&job_manager).await;
        return Ok(());
    }
    
    if args.foreground {
        info!("Running in foreground mode");
        
        // Start signal handlers
        setup_signal_handlers(job_manager.clone()).await?;
        
        // Keep daemon running
        tokio::select! {
            _ = event_handle => {
                warn!("Event processor stopped");
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Received Ctrl+C, shutting down");
            }
        }
    } else {
        info!("Daemon mode - use control tool to manage jobs");
        // TODO: Implement daemonization
    }
    
    Ok(())
}

async fn load_jobs_from_directory(job_manager: &JobManager, config_dir: &PathBuf) -> Result<()> {
    info!("Loading jobs from: {}", config_dir.display());
    
    if !config_dir.exists() {
        warn!("Config directory does not exist: {}", config_dir.display());
        return Ok(());
    }
    
    let mut loaded = 0;
    let mut failed = 0;
    
    match std::fs::read_dir(config_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                
                if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                    match job::config::JobConfig::from_file(&path).await {
                        Ok(config) => {
                            if let Err(e) = job_manager.load_job(config).await {
                                error!("Failed to load job from {}: {}", path.display(), e);
                                failed += 1;
                            } else {
                                loaded += 1;
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse config file {}: {}", path.display(), e);
                            failed += 1;
                        }
                    }
                }
            }
        }
        Err(e) => {
            error!("Failed to read config directory: {}", e);
        }
    }
    
    info!("Loaded {} jobs ({} failed)", loaded, failed);
    Ok(())
}

async fn show_daemon_status(job_manager: &JobManager) {
    let jobs = job_manager.list_jobs().await;
    
    println!("NusaLaunchd Daemon Status");
    println!("=========================");
    println!("Total jobs: {}", jobs.len());
    
    for job in jobs {
        let state_str = match job.state {
            job::JobState::Running => "✓".to_string(),
            job::JobState::Stopped => "✗".to_string(),
            job::JobState::Failed(ref reason) => format!("⚠ ({})", reason),
            _ => "?".to_string(),
        };
        
        println!("  {} {} [{}]", state_str, job.label, job.state);
    }
}

async fn handle_job_command(
    job_command: cli::args::JobCommands,
    _args: &CliArgs,
) -> Result<()> {
    match job_command {
        cli::args::JobCommands::Start { labels, wait, timeout } => {
            info!("Starting jobs: {:?}", labels);
            // TODO: Implement job starting
            Ok(())
        }
        _ => {
            warn!("Job command not fully implemented yet");
            Ok(())
        }
    }
}

async fn validate_config(path: PathBuf, strict: bool) -> Result<()> {
    info!("Validating config: {}", path.display());
    
    if path.is_dir() {
        // Validate all .toml files in directory
        let mut valid = 0;
        let mut invalid = 0;
        
        match std::fs::read_dir(&path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let file_path = entry.path();
                    if file_path.extension().and_then(|s| s.to_str()) == Some("toml") {
                        match job::config::JobConfig::from_file(&file_path).await {
                            Ok(config) => {
                                println!("✓ {}: {}", file_path.display(), config.label);
                                valid += 1;
                            }
                            Err(e) => {
                                println!("✗ {}: {}", file_path.display(), e);
                                invalid += 1;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to read directory: {}", e);
                return Err(util::error::NusaError::System(e.to_string()));
            }
        }
        
        println!("\nValidation complete: {} valid, {} invalid", valid, invalid);
        if strict && invalid > 0 {
            return Err(util::error::NusaError::System("Strict validation failed".into()));
        }
    } else {
        // Validate single file
        match job::config::JobConfig::from_file(&path).await {
            Ok(config) => {
                println!("✓ Configuration is valid");
                println!("  Label: {}", config.label);
                println!("  Program: {}", config.program.path.display());
                println!("  Supervision: keep_alive={}", config.supervision.keep_alive);
            }
            Err(e) => {
                println!("✗ Configuration is invalid: {}", e);
                if strict {
                    return Err(e);
                }
            }
        }
    }
    
    Ok(())
}

async fn show_status(_detailed: bool, _watch: bool, _format: cli::args::OutputFormat) -> Result<()> {
    // TODO: Implement status display
    println!("Status command not fully implemented yet");
    Ok(())
}

async fn generate_example(
    example_type: cli::args::ExampleType,
    output: Option<PathBuf>,
) -> Result<()> {
    let example = match example_type {
        cli::args::ExampleType::Simple => {
            include_str!("../configs/examples/simple.toml")
        }
        cli::args::ExampleType::WebServer => {
            include_str!("../configs/examples/web_server.toml")
        }
        cli::args::ExampleType::Database => {
            "# Database service example\nlabel = \"database\"\n\n[program]\npath = \"/usr/bin/postgres\"\n"
        }
        cli::args::ExampleType::Cron => {
            "# Cron-like service example\nlabel = \"cron-job\"\n\n[program]\npath = \"/usr/bin/bash\"\narguments = [\"-c\", \"echo 'Hello from cron'\"]\n"
        }
        cli::args::ExampleType::Socket => {
            "# Socket-activated service example\nlabel = \"socket-service\"\n\n[program]\npath = \"/usr/bin/echo\"\n# Socket configuration will be added in Week 3\n"
        }
    };
    
    if let Some(output_path) = output {
        std::fs::write(&output_path, example)
            .map_err(|e| util::error::NusaError::System(format!("Failed to write file: {}", e)))?;
        println!("Example written to: {}", output_path.display());
    } else {
        println!("{}", example);
    }
    
    Ok(())
}

async fn handle_socket_command(
    _socket_command: cli::args::SocketCommands,
) -> Result<()> {
    // TODO: Implement socket commands (Week 3)
    println!("Socket commands will be implemented in Week 3");
    Ok(())
}

async fn setup_signal_handlers(job_manager: job::JobManager) -> Result<()> {
    use signal_hook::consts::{SIGTERM, SIGINT};
    use signal_hook_tokio::Signals;
    
    let mut signals = Signals::new(&[SIGTERM, SIGINT])
        .map_err(|e| util::error::NusaError::System(format!("Failed to setup signals: {}", e)))?;
    
    let handle = signals.handle();
    
    tokio::spawn(async move {
        while let Some(signal) = signals.next().await {
            match signal {
                SIGTERM | SIGINT => {
                    info!("Received signal {}, shutting down gracefully", signal);
                    // TODO: Graceful shutdown of all jobs
                    break;
                }
                _ => {}
            }
        }
        handle.close();
    });
    
    Ok(())
}

fn init_logging(level: &str) {
    let filter = match level.to_lowercase().as_str() {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    };
    
    tracing_subscriber::fmt()
        .with_max_level(filter)
        .with_target(false)
        .init();
}