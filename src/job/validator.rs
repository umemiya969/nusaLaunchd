use crate::job::config::JobConfig;
use crate::util::error::{ConfigError, Result};
use std::path::Path;

pub struct ConfigValidator;

impl ConfigValidator {
    /// Validate a job configuration
    pub async fn validate(config: &JobConfig) -> Result<()> {
        // Check label
        Self::validate_label(&config.label)?;
        
        // Check program path
        Self::validate_program_path(&config.program.path)?;
        
        // Check working directory if specified
        if let Some(working_dir) = &config.working_directory {
            Self::validate_working_directory(working_dir)?;
        }
        
        // Check environment variables
        Self::validate_environment(&config.environment)?;
        
        // Check supervision settings
        Self::validate_supervision(&config.supervision)?;
        
        Ok(())
    }
    
    fn validate_label(label: &str) -> Result<()> {
        if label.trim().is_empty() {
            return Err(ConfigError::Validation("Label cannot be empty".into()).into());
        }
        
        // Check for invalid characters
        let invalid_chars = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
        if label.chars().any(|c| invalid_chars.contains(&c)) {
            return Err(ConfigError::Validation(
                format!("Label contains invalid characters: {}", label)
            ).into());
        }
        
        // Check length
        if label.len() > 256 {
            return Err(ConfigError::Validation(
                "Label too long (max 256 characters)".into()
            ).into());
        }
        
        Ok(())
    }
    
    fn validate_program_path(path: &std::path::Path) -> Result<()> {
        if path.to_string_lossy().is_empty() {
            return Err(ConfigError::Validation("Program path cannot be empty".into()).into());
        }
        
        // Check if path is absolute
        if !path.is_absolute() {
            return Err(ConfigError::Validation(
                format!("Program path must be absolute: {}", path.display())
            ).into());
        }
        
        // Note: We don't check if the file actually exists here
        // because the program might be installed later
        
        Ok(())
    }
    
    fn validate_working_directory(path: &std::path::Path) -> Result<()> {
        if !path.is_absolute() {
            return Err(ConfigError::Validation(
                format!("Working directory must be absolute: {}", path.display())
            ).into());
        }
        
        Ok(())
    }
    
    fn validate_environment(env_vars: &[crate::job::config::EnvironmentVar]) -> Result<()> {
        for env in env_vars {
            if env.key.trim().is_empty() {
                return Err(ConfigError::Validation(
                    "Environment variable key cannot be empty".into()
                ).into());
            }
            
            // Check for basic validity
            if env.key.contains('=') || env.key.contains('\0') {
                return Err(ConfigError::Validation(
                    format!("Invalid environment variable key: {}", env.key)
                ).into());
            }
        }
        
        Ok(())
    }
    
    fn validate_supervision(supervision: &crate::job::config::SupervisionConfig) -> Result<()> {
        // Validate restart delay
        if supervision.restart_delay_sec > 3600 {
            return Err(ConfigError::Validation(
                "Restart delay too long (max 3600 seconds)".into()
            ).into());
        }
        
        Ok(())
    }
    
    /// Validate a configuration file without loading it
    pub async fn validate_file<P: AsRef<Path>>(path: P) -> Result<JobConfig> {
        let config = JobConfig::from_file(path).await?;
        Self::validate(&config).await?;
        Ok(config)
    }
}