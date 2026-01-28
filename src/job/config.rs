use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::util::error::{ConfigError, Result};

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct JobConfig {
    /// Unique identifier for the job
    pub label: String,
    
    /// Human-readable description
    #[serde(default)]
    pub description: Option<String>,
    
    /// Program to execute
    pub program: ProgramConfig,
    
    /// Process supervision settings
    #[serde(default)]
    pub supervision: SupervisionConfig,
    
    /// Environment variables
    #[serde(default)]
    pub environment: Vec<EnvironmentVar>,
    
    /// Working directory
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProgramConfig {
    /// Path to executable
    pub path: PathBuf,
    
    /// Command line arguments
    #[serde(default)]
    pub arguments: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct SupervisionConfig {
    /// Whether to keep the process alive
    #[serde(default = "default_true")]
    pub keep_alive: bool,
    
    /// Restart policy
    #[serde(default)]
    pub restart_policy: RestartPolicy,
    
    /// Seconds to wait before restarting
    #[serde(default = "default_restart_delay")]
    pub restart_delay_sec: u64,
    
    /// Maximum restart attempts (0 = unlimited)
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    Never,      // Never restart
    Always,     // Always restart
    OnFailure,  // Restart on non-zero exit
    OnCrash,    // Restart on signal termination
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self::OnFailure
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EnvironmentVar {
    pub key: String,
    pub value: String,
}

// Default value helpers
fn default_true() -> bool { true }
fn default_restart_delay() -> u64 { 1 }
fn default_max_restarts() -> u32 { 5 }

impl JobConfig {
    /// Load job configuration from a TOML file
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        
        // Check if file exists
        if !path.exists() {
            return Err(ConfigError::FileNotFound(path.to_path_buf()).into());
        }
        
        // Read file content
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::Parse(format!("Failed to read file: {}", e)))?;
        
        // Parse TOML
        let config: Self = toml::from_str(&content)
            .map_err(|e| ConfigError::Parse(format!("Invalid TOML: {}", e)))?;
        
        // Validate the configuration
        config.validate()?;
        
        Ok(config)
    }
    
    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Check if label is not empty
        if self.label.trim().is_empty() {
            return Err(ConfigError::Validation("Label cannot be empty".into()).into());
        }
        
        // Check if program path is not empty
        if self.program.path.to_string_lossy().is_empty() {
            return Err(ConfigError::Validation("Program path cannot be empty".into()).into());
        }
        
        // TODO: Check if program exists (but only warn, not error)
        // karena mungkin di-deploy nanti
        
        // Validate restart policy logic
        if !self.supervision.keep_alive && self.supervision.restart_policy != RestartPolicy::Never {
            tracing::warn!(
                "Job '{}': restart_policy is ignored when keep_alive=false",
                self.label
            );
        }
        
        Ok(())
    }
    
    /// Convert to environment variables format for std::process
    pub fn get_env_vars(&self) -> Vec<(String, String)> {
        self.environment
            .iter()
            .map(|env| (env.key.clone(), env.value.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    
    #[test]
    fn test_load_valid_config() {
        let toml_content = r#"
            label = "test-service"
            description = "A test service"
            
            [program]
            path = "/usr/bin/echo"
            arguments = ["Hello, World!"]
            
            [supervision]
            keep_alive = false
            restart_policy = "never"
        "#;
        
        let mut file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut file, toml_content.as_bytes()).unwrap();
        
        let config = JobConfig::from_file(file.path()).unwrap();
        
        assert_eq!(config.label, "test-service");
        assert_eq!(config.program.path, PathBuf::from("/usr/bin/echo"));
        assert_eq!(config.supervision.keep_alive, false);
    }
    
    #[test]
    fn test_invalid_config() {
        let toml_content = r#"
            label = ""
            [program]
            path = ""
        "#;
        
        let mut file = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut file, toml_content.as_bytes()).unwrap();
        
        let result = JobConfig::from_file(file.path());
        assert!(result.is_err());
    }
}