use std::collections::HashMap;
use std::path::PathBuf;
use tokio::process::{Command, Child};
use tracing::{info, warn, debug, instrument};
use crate::job::config::{JobConfig, RestartPolicy};
use crate::event::dispatcher::EventDispatcher;
use crate::util::error::{NusaError, Result};

pub struct ProcessSpawner {
    event_dispatcher: EventDispatcher,
}

impl ProcessSpawner {
    pub fn new(event_dispatcher: EventDispatcher) -> Self {
        Self { event_dispatcher }
    }
    
    /// Spawn a process based on job configuration
    #[instrument(skip(self, config), fields(job = %config.label))]
    pub async fn spawn(&self, config: &JobConfig) -> Result<(u32, tokio::task::JoinHandle<()>)> {
        debug!("Spawning process: {:?}", config.program.path);
        
        let mut command = Command::new(&config.program.path);
        
        // Set command arguments
        if !config.program.arguments.is_empty() {
            command.args(&config.program.arguments);
        }
        
        // Set environment variables
        for env in &config.environment {
            command.env(&env.key, &env.value);
        }
        
        // Set working directory
        if let Some(working_dir) = &config.working_directory {
            if working_dir.exists() {
                command.current_dir(working_dir);
            } else {
                warn!("Working directory does not exist: {:?}", working_dir);
            }
        }
        
        // Setup stdio
        // TODO: Implement proper logging to files/journal
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        
        // Spawn the process
        let mut child = command.spawn()
            .map_err(|e| {
                NusaError::Process(format!("Failed to spawn process '{}': {}", 
                    config.program.path.display(), e))
            })?;
        
        let pid = child.id()
            .ok_or_else(|| NusaError::Process("Failed to get PID".into()))?;
        
        info!("Process spawned [PID: {}] for job: {}", pid, config.label);
        
        // Create monitor task
        let label = config.label.clone();
        let config_clone = config.clone();
        let event_dispatcher = self.event_dispatcher.clone();
        
        let handle = tokio::spawn(async move {
            Self::monitor_process(
                label,
                config_clone,
                child,
                event_dispatcher
            ).await;
        });
        
        Ok((pid, handle))
    }
    
    /// Monitor a running process and handle its exit
    #[instrument(skip(child, event_dispatcher), fields(job = %label))]
    async fn monitor_process(
        label: String,
        config: JobConfig,
        mut child: Child,
        event_dispatcher: EventDispatcher,
    ) {
        debug!("Starting process monitor");
        
        match child.wait().await {
            Ok(status) => {
                let exit_code = status.code().unwrap_or(-1);
                let signal = status.signal();
                
                debug!("Process exited: code={}, signal={:?}", exit_code, signal);
                
                // Determine if restart is needed
                let restart_needed = if config.supervision.keep_alive {
                    match config.supervision.restart_policy {
                        RestartPolicy::Always => true,
                        RestartPolicy::Never => false,
                        RestartPolicy::OnFailure => exit_code != 0,
                        RestartPolicy::OnCrash => signal.is_some(),
                    }
                } else {
                    false
                };
                
                // Send exit event
                let _ = event_dispatcher.send(crate::job::manager::JobEvent::JobExited(
                    label.clone(),
                    exit_code,
                    signal,
                    0, // restart_count will be updated by manager
                )).await;
                
                // If restart needed, signal the manager
                if restart_needed {
                    let _ = event_dispatcher.send(
                        crate::job::manager::JobEvent::JobReadyForRestart(label)
                    ).await;
                }
            }
            Err(e) => {
                warn!("Error monitoring process for job '{}': {}", label, e);
                
                let _ = event_dispatcher.send(crate::job::manager::JobEvent::JobFailed(
                    label,
                    crate::job::manager::JobState::Failed(format!("Monitor error: {}", e)),
                )).await;
            }
        }
    }
    
    /// Kill a process with escalating signals
    pub async fn kill_process(pid: u32, force: bool) -> Result<()> {
        let pid_i32 = pid as i32;
        
        if force {
            // Send SIGKILL immediately
            nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid_i32),
                nix::sys::signal::Signal::SIGKILL
            ).map_err(|e| NusaError::Process(format!("Failed to send SIGKILL: {}", e)))?;
        } else {
            // Try SIGTERM first
            nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid_i32),
                nix::sys::signal::Signal::SIGTERM
            ).map_err(|e| {
                warn!("Failed to send SIGTERM to PID {}: {}", pid, e);
                // If SIGTERM fails, try SIGKILL
                nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid_i32),
                    nix::sys::signal::Signal::SIGKILL
                )
            }).map_err(|e| NusaError::Process(format!("Failed to kill process: {}", e)))?;
        }
        
        Ok(())
    }
}

impl Clone for ProcessSpawner {
    fn clone(&self) -> Self {
        Self {
            event_dispatcher: self.event_dispatcher.clone(),
        }
    }
}