use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::process::Command;
use tracing::{info, warn, error, debug};

use crate::job::config::{JobConfig, RestartPolicy};
use crate::util::error::{NusaError, ProcessError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Restarting,
    Failed(String),
}

#[derive(Debug)]
pub struct JobInstance {
    pub config: JobConfig,
    pub state: JobState,
    pub pid: Option<u32>,
    pub start_time: Option<std::time::SystemTime>,
    pub restart_count: u32,
    pub exit_code: Option<i32>,
}

pub struct JobManager {
    jobs: Arc<RwLock<HashMap<String, JobInstance>>>,
    event_tx: tokio::sync::mpsc::Sender<JobEvent>,
}

impl JobManager {
    /// Create a new JobManager
    pub fn new() -> (Self, tokio::sync::mpsc::Receiver<JobEvent>) {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(100);
        
        let manager = Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
        };
        
        (manager, event_rx)
    }
    
    /// Load a job configuration
    pub async fn load_job(&self, config: JobConfig) -> Result<()> {
        let label = config.label.clone();
        
        let mut jobs = self.jobs.write().await;
        
        // Check if job already exists
        if jobs.contains_key(&label) {
            return Err(NusaError::JobExists(label));
        }
        
        // Create job instance
        let instance = JobInstance {
            config: config.clone(),
            state: JobState::Stopped,
            pid: None,
            start_time: None,
            restart_count: 0,
            exit_code: None,
        };
        
        jobs.insert(label.clone(), instance);
        
        // Send event
        self.event_tx.send(JobEvent::JobLoaded(label.clone())).await
            .map_err(|e| NusaError::System(format!("Failed to send event: {}", e)))?;
        
        info!("Loaded job: {}", label);
        
        // Start if RunAtLoad equivalent (we'll implement this later)
        if config.supervision.keep_alive {
            self.start_job(&label).await?;
        }
        
        Ok(())
    }
    
    /// Start a job
    pub async fn start_job(&self, label: &str) -> Result<()> {
        let mut jobs = self.jobs.write().await;
        
        let instance = jobs.get_mut(label)
            .ok_or_else(|| NusaError::JobNotFound(label.to_string()))?;
        
        // Check if already running
        if matches!(instance.state, JobState::Running | JobState::Starting) {
            warn!("Job '{}' is already running", label);
            return Ok(());
        }
        
        // Update state
        instance.state = JobState::Starting;
        
        // Drop write lock to avoid deadlock while spawning
        let config = instance.config.clone();
        drop(jobs);
        
        // Spawn process
        let pid = self.spawn_process(&config).await?;
        
        // Update instance with PID
        let mut jobs = self.jobs.write().await;
        let instance = jobs.get_mut(label).unwrap();
        
        instance.state = JobState::Running;
        instance.pid = Some(pid);
        instance.start_time = Some(std::time::SystemTime::now());
        
        // Send event
        self.event_tx.send(JobEvent::JobStarted(label.to_string(), pid)).await
            .map_err(|e| NusaError::System(format!("Failed to send event: {}", e)))?;
        
        info!("Started job '{}' with PID {}", label, pid);
        
        Ok(())
    }
    
    /// Stop a job
    pub async fn stop_job(&self, label: &str) -> Result<()> {
        let mut jobs = self.jobs.write().await;
        
        let instance = jobs.get_mut(label)
            .ok_or_else(|| NusaError::JobNotFound(label.to_string()))?;
        
        // Update state
        instance.state = JobState::Stopping;
        
        // Get PID
        let pid = instance.pid;
        
        drop(jobs);
        
        // Send SIGTERM if running
        if let Some(pid) = pid {
            if let Err(e) = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGTERM
            ) {
                warn!("Failed to send SIGTERM to job '{}' (PID {}): {}", label, pid, e);
            }
            
            // TODO: Implement graceful shutdown with timeout
            // then SIGKILL if needed
        }
        
        // Update state
        let mut jobs = self.jobs.write().await;
        let instance = jobs.get_mut(label).unwrap();
        
        instance.state = JobState::Stopped;
        instance.pid = None;
        
        self.event_tx.send(JobEvent::JobStopped(label.to_string())).await
            .map_err(|e| NusaError::System(format!("Failed to send event: {}", e)))?;
        
        info!("Stopped job: {}", label);
        
        Ok(())
    }
    
    /// Get job status
    pub async fn get_job_status(&self, label: &str) -> Option<JobStatus> {
        let jobs = self.jobs.read().await;
        jobs.get(label).map(|instance| JobStatus {
            label: label.to_string(),
            state: instance.state.clone(),
            pid: instance.pid,
            restart_count: instance.restart_count,
            uptime: instance.start_time.map(|t| {
                t.elapsed().unwrap_or_default()
            }),
        })
    }
    
    /// List all jobs
    pub async fn list_jobs(&self) -> Vec<JobStatus> {
        let jobs = self.jobs.read().await;
        jobs.iter()
            .map(|(label, instance)| JobStatus {
                label: label.clone(),
                state: instance.state.clone(),
                pid: instance.pid,
                restart_count: instance.restart_count,
                uptime: instance.start_time.map(|t| {
                    t.elapsed().unwrap_or_default()
                }),
            })
            .collect()
    }
    
    /// Spawn a process based on job configuration
    async fn spawn_process(&self, config: &JobConfig) -> Result<u32> {
        debug!("Spawning process for job: {}", config.label);
        
        let mut command = Command::new(&config.program.path);
        
        // Set arguments
        if !config.program.arguments.is_empty() {
            command.args(&config.program.arguments);
        }
        
        // Set environment variables
        for (key, value) in config.get_env_vars() {
            command.env(key, value);
        }
        
        // Set working directory
        if let Some(working_dir) = &config.working_directory {
            command.current_dir(working_dir);
        }
        
        // Redirect stdio to null for now
        // TODO: Implement proper logging
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        
        // Spawn the process
        let child = command.spawn()
            .map_err(|e| ProcessError::Spawn(format!("Failed to spawn process: {}", e)))?;
        
        let pid = child.id()
            .ok_or_else(|| ProcessError::Spawn("Failed to get PID".into()))?;
        
        // Spawn monitor task
        let label = config.label.clone();
        let config_clone = config.clone();
        let event_tx = self.event_tx.clone();
        
        tokio::spawn(async move {
            Self::monitor_process(label, config_clone, child, event_tx).await;
        });
        
        Ok(pid)
    }
    
    /// Monitor a running process
    async fn monitor_process(
        label: String,
        config: JobConfig,
        mut child: tokio::process::Child,
        event_tx: tokio::sync::mpsc::Sender<JobEvent>,
    ) {
        debug!("Starting monitor for job: {}", label);
        
        match child.wait().await {
            Ok(status) => {
                let exit_code = status.code().unwrap_or(-1);
                let signal = if let Some(code) = status.signal() {
                    Some(code)
                } else {
                    None
                };
                
                // Send exit event
                let _ = event_tx.send(JobEvent::JobExited(
                    label.clone(),
                    exit_code,
                    signal,
                )).await;
                
                // Handle restart policy
                if config.supervision.keep_alive {
                    let should_restart = match config.supervision.restart_policy {
                        RestartPolicy::Always => true,
                        RestartPolicy::Never => false,
                        RestartPolicy::OnFailure => exit_code != 0,
                        RestartPolicy::OnCrash => signal.is_some(),
                    };
                    
                    if should_restart {
                        debug!("Job '{}' will restart (exit code: {})", label, exit_code);
                        let _ = event_tx.send(JobEvent::JobRestartNeeded(label)).await;
                    }
                }
            }
            Err(e) => {
                error!("Error monitoring job '{}': {}", label, e);
                let _ = event_tx.send(JobEvent::JobFailed(label, e.to_string())).await;
            }
        }
    }
}

#[derive(Debug)]
pub struct JobStatus {
    pub label: String,
    pub state: JobState,
    pub pid: Option<u32>,
    pub restart_count: u32,
    pub uptime: Option<std::time::Duration>,
}

#[derive(Debug)]
pub enum JobEvent {
    JobLoaded(String),
    JobStarted(String, u32),
    JobStopped(String),
    JobExited(String, i32, Option<i32>),
    JobFailed(String, String),
    JobRestartNeeded(String),
}

impl std::fmt::Display for JobState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobState::Stopped => write!(f, "stopped"),
            JobState::Starting => write!(f, "starting"),
            JobState::Running => write!(f, "running"),
            JobState::Stopping => write!(f, "stopping"),
            JobState::Restarting => write!(f, "restarting"),
            JobState::Failed(reason) => write!(f, "failed ({})", reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_job_lifecycle() {
        let (manager, mut event_rx) = JobManager::new();
        
        let config = JobConfig {
            label: "test-job".to_string(),
            description: Some("Test job".to_string()),
            program: ProgramConfig {
                path: std::env::current_exe().unwrap(), // Use test binary as "program"
                arguments: vec!["--help".to_string()],
            },
            supervision: SupervisionConfig {
                keep_alive: false,
                restart_policy: RestartPolicy::Never,
                restart_delay_sec: 1,
                max_restarts: 0,
            },
            environment: vec![],
            working_directory: None,
        };
        
        // Load job
        manager.load_job(config).await.unwrap();
        
        // Check job exists
        let status = manager.get_job_status("test-job").await.unwrap();
        assert_eq!(status.label, "test-job");
        assert!(matches!(status.state, JobState::Stopped));
        
        // Verify event was sent
        let event = event_rx.recv().await.unwrap();
        assert!(matches!(event, JobEvent::JobLoaded(label) if label == "test-job"));
    }
}