use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum NusaError {
    #[error("Config error: {0}")]
    Config(#[from] ConfigError),
    
    #[error("Process error: {0}")]
    Process(#[from] ProcessError),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Job '{0}' not found")]
    JobNotFound(String),
    
    #[error("Job '{0}' already exists")]
    JobExists(String),
    
    #[error("System error: {0}")]
    System(String),
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to parse config: {0}")]
    Parse(String),
    
    #[error("Invalid config: {0}")]
    Validation(String),
    
    #[error("File '{0}' not found")]
    FileNotFound(PathBuf),
    
    #[error("Unsupported config format")]
    UnsupportedFormat,
    
    #[error("TOML parsing error: {0}")]
    Toml(#[from] toml::de::Error),
}

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("Failed to spawn process: {0}")]
    Spawn(String),
    
    #[error("Process exited with code {0}")]
    Exit(i32),
    
    #[error("Process terminated by signal {0}")]
    Signal(i32),
    
    #[error("Process timeout")]
    Timeout,
    
    #[error("Process error: {0}")]
    Other(String),
}

impl From<String> for ProcessError {
    fn from(s: String) -> Self {
        ProcessError::Other(s)
    }
}

pub type Result<T> = std::result::Result<T, NusaError>;