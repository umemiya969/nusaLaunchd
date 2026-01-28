use std::path::PathBuf;
use clap::Parser;
use tracing::{info, error, warn};
use tracing_subscriber;

mod job;
mod util;

use job::{JobManager, JobEvent};
use util::error::{Result, NusaError};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Directory containing job configuration files
    #[arg(short, long, default_value = "/etc/nusalaunchd/jobs")]
    config_dir: PathBuf,
    
    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,
    
    /// Run in foreground (don't daemonize)
    #[arg(short, long)]
    foreground: bool,
    
    /// Configuration file to load
    #[arg(short = 'f', long)]
    config_file: Option<PathBuf>,
}

struct NusaLaunchd {
    job_manager: JobManager,
    event_rx: tokio::sync::mpsc::Receiver<JobEvent>,
}

impl NusaLaunchd {
    fn new() -> Self {
        let (job_manager, event_rx) = JobManager::new();
        Self {
            job_manager,
            event_rx,
        }
    }
    
    /// Load all job configurations from a directory
    async fn load_jobs_from_dir(&self, dir: &PathBuf) -> Result<()> {
        info!("Loading jobs from directory: {}", dir.display());
        
        if !dir.exists() {
            warn!("Config directory does not exist: {}", dir.display());
            return Ok(());
        }
        
        let mut loaded = 0;
        let mut failed = 0;
        
        // Read all .toml files in the directory
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    
                    // Only process .toml files
                    if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                        match job::config::JobConfig::from_file(&path) {
                            Ok(config) => {
                                if let Err(e) = self.job_manager.load_job(config).await {
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
                return Err(NusaError::System(
                    format!("Failed to read config directory: {}", e)
                ));
            }
        }
        
        info!("Loaded {} jobs ({} failed)", loaded, failed);
        Ok(())
    }
    
    /// Load a single job configuration file
    async fn load_job_from_file(&self, file: &PathBuf) -> Result<()> {
        info!("Loading job from file: {}", file.display());
        
        let config = job::config::JobConfig::from_file(file)?;
        self.job_manager.load_job(config).await?;
        
        Ok(())
    }
    
    /// Main event loop
    async fn run_event_loop(mut self) -> Result<()> {
        info!("Starting NusaLaunchd event loop");
        
        // Simple event loop for now
        while let Some(event) = self.event_rx.recv().await {
            self.handle_event(event).await?;
        }
        
        Ok(())
    }
    
    /// Handle job events
    async fn handle_event(&self, event: JobEvent) -> Result<()> {
        match event {
            JobEvent::JobLoaded(label) => {
                info!("Job loaded: {}", label);
            }
            JobEvent::JobStarted(label, pid) => {
                info!("Job started: {} [PID: {}]", label, pid);
            }
            JobEvent::JobStopped(label) => {
                info!("Job stopped: {}", label);
            }
            JobEvent::JobExited(label, code, signal) => {
                if let Some(sig) = signal {
                    warn!("Job exited: {} [signal: {}]", label, sig);
                } else {
                    info!("Job exited: {} [code: {}]", label, code);
                }
            }
            JobEvent::JobFailed(label, reason) => {
                error!("Job failed: {} [reason: {}]", label, reason);
            }
            JobEvent::JobRestartNeeded(label) => {
                warn!("Job needs restart: {}", label);
                // TODO: Implement restart logic with delays and limits
            }
        }
        
        Ok(())
    }
    
    /// Print current status of all jobs
    async fn print_status(&self) {
        let jobs = self.job_manager.list_jobs().await;
        
        println!("NusaLaunchd Status");
        println!("==================");
        println!("{:<20} {:<12} {:<8} {:<10}", "LABEL", "STATE", "PID", "UPTIME");
        println!("{:-<20} {:-<12} {:-<8} {:-<10}", "", "", "", "");
        
        for job in jobs {
            let pid_str = match job.pid {
                Some(pid) => pid.to_string(),
                None => "-".to_string(),
            };
            
            let uptime_str = match job.uptime {
                Some(duration) => {
                    let secs = duration.as_secs();
                    if secs < 60 {
                        format!("{}s", secs)
                    } else if secs < 3600 {
                        format!("{}m", secs / 60)
                    } else {
                        format!("{}h", secs / 3600)
                    }
                }
                None => "-".to_string(),
            };
            
            println!("{:<20} {:<12} {:<8} {:<10}", 
                job.label, 
                job.state.to_string(),
                pid_str,
                uptime_str
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();
    
    // Initialize logging
    init_logging(&args.log_level);
    
    info!("Starting NusaLaunchd v{}", env!("CARGO_PKG_VERSION"));
    
    // Create application instance
    let app = NusaLaunchd::new();
    
    // Load configurations
    if let Some(config_file) = &args.config_file {
        app.load_job_from_file(config_file).await?;
    } else {
        app.load_jobs_from_dir(&args.config_dir).await?;
    }
    
    // Print initial status
    app.print_status().await;
    
    // Run event loop if in foreground mode
    if args.foreground {
        info!("Running in foreground mode");
        app.run_event_loop().await?;
    } else {
        info!("Run with --foreground to start the event loop");
        info!("Use control tool to manage jobs (coming soon)");
    }
    
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