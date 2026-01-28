use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "nusalaunchd",
    version,
    about = "A launchd-style init system for Linux",
    long_about = "NusaLaunchd is a modern init system for Linux inspired by macOS launchd, \
                 providing socket activation, dependency management, and process supervision."
)]
pub struct CliArgs {
    #[command(subcommand)]
    pub command: Option<Commands>,
    
    /// Directory containing job configuration files
    #[arg(
        short = 'c',
        long = "config-dir",
        default_value = "/etc/nusalaunchd/jobs",
        global = true
    )]
    pub config_dir: PathBuf,
    
    /// Log level
    #[arg(
        short = 'l',
        long = "log-level",
        value_enum,
        default_value = "info",
        global = true
    )]
    pub log_level: LogLevel,
    
    /// Run in foreground (don't daemonize)
    #[arg(short = 'f', long = "foreground", global = true)]
    pub foreground: bool,
    
    /// Configuration file to test/validate
    #[arg(short = 't', long = "test", global = true)]
    pub test_config: Option<PathBuf>,
    
    /// Dry run - don't actually start jobs
    #[arg(long = "dry-run", global = true)]
    pub dry_run: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the NusaLaunchd daemon
    Daemon {
        /// Daemon-specific options
        #[command(flatten)]
        daemon_opts: DaemonOptions,
    },
    
    /// Control and query jobs
    Job {
        #[command(subcommand)]
        job_command: JobCommands,
    },
    
    /// Validate configuration files
    Validate {
        /// Configuration file or directory to validate
        path: PathBuf,
        
        /// Strict validation (treat warnings as errors)
        #[arg(short = 's', long = "strict")]
        strict: bool,
    },
    
    /// Generate example configuration
    Example {
        /// Type of example to generate
        #[arg(value_enum)]
        example_type: ExampleType,
        
        /// Output file (default: stdout)
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
    },
    
    /// Show system status
    Status {
        /// Show detailed information
        #[arg(short = 'd', long = "detailed")]
        detailed: bool,
        
        /// Watch mode (continuously update)
        #[arg(short = 'w', long = "watch")]
        watch: bool,
        
        /// Output format
        #[arg(short = 'f', long = "format", value_enum, default_value = "table")]
        format: OutputFormat,
    },
    
    /// Manage the control socket
    Socket {
        #[command(subcommand)]
        socket_command: SocketCommands,
    },
}

#[derive(Parser, Debug)]
pub struct DaemonOptions {
    /// PID file location
    #[arg(long = "pid-file", default_value = "/run/nusalaunchd.pid")]
    pid_file: PathBuf,
    
    /// State directory
    #[arg(long = "state-dir", default_value = "/var/lib/nusalaunchd")]
    state_dir: PathBuf,
    
    /// Runtime directory
    #[arg(long = "runtime-dir", default_value = "/run/nusalaunchd")]
    runtime_dir: PathBuf,
    
    /// Maximum number of jobs
    #[arg(long = "max-jobs", default_value = "512")]
    max_jobs: usize,
}

impl Default for DaemonOptions {
    fn default() -> Self {
        Self {
            pid_file: PathBuf::from("/run/nusalaunchd.pid"),
            state_dir: PathBuf::from("/var/lib/nusalaunchd"),
            runtime_dir: PathBuf::from("/run/nusalaunchd"),
            max_jobs: 512,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum JobCommands {
    /// Start a job
    Start {
        /// Job label(s)
        labels: Vec<String>,
        
        /// Wait for job to fully start
        #[arg(short = 'w', long = "wait")]
        wait: bool,
        
        /// Timeout in seconds for wait
        #[arg(long = "timeout", default_value = "30")]
        timeout: u64,
    },
    
    /// Stop a job
    Stop {
        /// Job label(s)
        labels: Vec<String>,
        
        /// Force stop (SIGKILL)
        #[arg(short = 'f', long = "force")]
        force: bool,
        
        /// Timeout before force stop
        #[arg(long = "timeout", default_value = "10")]
        timeout: u64,
    },
    
    /// Restart a job
    Restart {
        /// Job label(s)
        labels: Vec<String>,
        
        /// Skip if not running
        #[arg(long = "skip-if-stopped")]
        skip_if_stopped: bool,
    },
    
    /// Show job status
    Status {
        /// Job label (optional, shows all if omitted)
        label: Option<String>,
        
        /// Show full configuration
        #[arg(short = 'c', long = "config")]
        show_config: bool,
        
        /// Show process tree
        #[arg(short = 't', long = "tree")]
        show_tree: bool,
    },
    
    /// Enable job at boot
    Enable {
        /// Job label(s)
        labels: Vec<String>,
        
        /// Target (default, multi-user, graphical)
        #[arg(short = 't', long = "target", default_value = "multi-user")]
        target: String,
        
        /// Now (start immediately)
        #[arg(short = 'n', long = "now")]
        now: bool,
    },
    
    /// Disable job at boot
    Disable {
        /// Job label(s)
        labels: Vec<String>,
        
        /// Stop if currently running
        #[arg(short = 's', long = "stop")]
        stop: bool,
    },
    
    /// Reload job configuration
    Reload {
        /// Job label(s)
        labels: Vec<String>,
        
        /// Restart if running
        #[arg(short = 'r', long = "restart")]
        restart: bool,
    },
    
    /// Follow job logs
    Logs {
        /// Job label
        label: String,
        
        /// Number of lines to show
        #[arg(short = 'n', long = "lines", default_value = "50")]
        lines: usize,
        
        /// Follow logs
        #[arg(short = 'f', long = "follow")]
        follow: bool,
        
        /// Show since timestamp
        #[arg(long = "since")]
        since: Option<String>,
        
        /// Show until timestamp
        #[arg(long = "until")]
        until: Option<String>,
    },
    
    /// List all jobs
    List {
        /// Filter by state
        #[arg(short = 's', long = "state")]
        state_filter: Option<String>,
        
        /// Show only loaded jobs
        #[arg(long = "loaded")]
        loaded_only: bool,
        
        /// Show only running jobs
        #[arg(long = "running")]
        running_only: bool,
        
        /// Show only failed jobs
        #[arg(long = "failed")]
        failed_only: bool,
        
        /// Output format
        #[arg(short = 'o', long = "output", value_enum, default_value = "table")]
        output_format: OutputFormat,
    },
}

#[derive(Subcommand, Debug)]
pub enum SocketCommands {
    /// Show socket status
    Status,
    
    /// Activate socket
    Activate {
        /// Socket name
        name: String,
    },
    
    /// Deactivate socket
    Deactivate {
        /// Socket name
        name: String,
    },
}

#[derive(ValueEnum, Clone, Debug)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum ExampleType {
    Simple,
    WebServer,
    Database,
    Cron,
    Socket,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
    Plain,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Error => write!(f, "error"),
            LogLevel::Warn => write!(f, "warn"),
            LogLevel::Info => write!(f, "info"),
            LogLevel::Debug => write!(f, "debug"),
            LogLevel::Trace => write!(f, "trace"),
        }
    }
}