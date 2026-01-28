use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::time;
use tracing::{info, warn, error, debug, instrument};

use crate::job::config::{JobConfig, RestartPolicy};
use crate::process::spawner::ProcessSpawner;
use crate::event::dispatcher::EventDispatcher;
use crate::util::error::{NusaError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Restarting,
    Failed(String),
    Backoff,  // Waiting before restart
}

#[derive(Debug)]
pub struct JobInstance {
    pub config: JobConfig,
    pub state: JobState,
    pub pid: Option<u32>,
    pub start_time: Option<Instant>,
    pub restart_count: u32,
    pub last_exit_code: Option<i32>,
    pub last_exit_signal: Option<i32>,
    pub backoff_until: Option<Instant>,
    pub process_handle: Option<tokio::task::JoinHandle<()>>,
}

pub struct JobManager {
    jobs: Arc<RwLock<HashMap<String, JobInstance>>>,
    event_dispatcher: EventDispatcher,
    spawner: ProcessSpawner,
    restart_tx: mpsc::Sender<RestartRequest>,
}

impl JobManager {
    /// Create a new JobManager
    pub async fn new() -> Result<(Self, mpsc::Receiver<JobEvent>)> {
        let (event_tx, event_rx) = mpsc::channel(100);
        let (restart_tx, restart_rx) = mpsc::channel(50);
        
        let event_dispatcher = EventDispatcher::new(event_tx);
        let spawner = ProcessSpawner::new(event_dispatcher.clone());
        
        let manager = Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            event_dispatcher: event_dispatcher.clone(),
            spawner,
            restart_tx,
        };
        
        // Start background tasks
        manager.start_background_tasks(restart_rx).await;
        
        Ok((manager, event_rx))
    }
    
    /// Start background tasks for restart handling
    async fn start_background_tasks(&self, mut restart_rx: mpsc::Receiver<RestartRequest>) {
        let jobs = Arc::clone(&self.jobs);
        let event_dispatcher = self.event_dispatcher.clone();
        
        tokio::spawn(async move {
            while let Some(request) = restart_rx.recv().await {
                handle_restart_request(
                    request,
                    Arc::clone(&jobs),
                    event_dispatcher.clone()
                ).await;
            }
        });
    }
    
    /// Load a job configuration
    #[instrument(skip(self), fields(job = %config.label))]
    pub async fn load_job(&self, config: JobConfig) -> Result<()> {
        let label = config.label.clone();
        
        debug!("Loading job configuration");
        
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
            last_exit_code: None,
            last_exit_signal: None,
            backoff_until: None,
            process_handle: None,
        };
        
        jobs.insert(label.clone(), instance);
        
        // Send event
        self.event_dispatcher.send(JobEvent::JobLoaded(label.clone())).await?;
        
        info!("Job loaded successfully: {}", label);
        
        // Start job if keep_alive is true (similar to RunAtLoad)
        if config.supervision.keep_alive {
            debug!("Auto-starting job due to keep_alive=true");
            // We'll start it asynchronously to avoid holding the lock
            let self_clone = self.clone();
            let label_clone = label.clone();
            tokio::spawn(async move {
                if let Err(e) = self_clone.start_job(&label_clone).await {
                    error!("Failed to auto-start job '{}': {}", label_clone, e);
                }
            });
        }
        
        Ok(())
    }
    
    /// Start a job
    #[instrument(skip(self), fields(job = %label))]
    pub async fn start_job(&self, label: &str) -> Result<()> {
        debug!("Starting job");
        
        let mut jobs = self.jobs.write().await;
        
        let instance = jobs.get_mut(label)
            .ok_or_else(|| NusaError::JobNotFound(label.to_string()))?;
        
        // Check current state
        match &instance.state {
            JobState::Running | JobState::Starting => {
                warn!("Job is already running or starting");
                return Ok(());
            }
            JobState::Backoff => {
                if let Some(until) = instance.backoff_until {
                    if Instant::now() < until {
                        let wait_secs = (until - Instant::now()).as_secs();
                        warn!("Job in backoff, waiting {} seconds", wait_secs);
                        return Ok(());
                    }
                }
                // Backoff expired, proceed
            }
            _ => {} // Other states are fine
        }
        
        // Update state
        instance.state = JobState::Starting;
        instance.backoff_until = None;
        
        // Drop write lock temporarily to spawn process
        let config = instance.config.clone();
        drop(jobs);
        
        // Spawn process
        match self.spawner.spawn(&config).await {
            Ok((pid, handle)) => {
                // Re-acquire lock and update instance
                let mut jobs = self.jobs.write().await;
                let instance = jobs.get_mut(label).unwrap();
                
                instance.state = JobState::Running;
                instance.pid = Some(pid);
                instance.start_time = Some(Instant::now());
                instance.process_handle = Some(handle);
                instance.restart_count = 0;
                
                self.event_dispatcher.send(JobEvent::JobStarted(
                    label.to_string(),
                    pid,
                    instance.start_time.unwrap()
                )).await?;
                
                info!("Job started successfully [PID: {}]", pid);
                Ok(())
            }
            Err(e) => {
                // Update state to failed
                let mut jobs = self.jobs.write().await;
                let instance = jobs.get_mut(label).unwrap();
                
                instance.state = JobState::Failed(format!("Failed to start: {}", e));
                
                error!("Failed to start job: {}", e);
                Err(e)
            }
        }
    }
    
    /// Stop a job
    #[instrument(skip(self), fields(job = %label))]
    pub async fn stop_job(&self, label: &str) -> Result<()> {
        debug!("Stopping job");
        
        let mut jobs = self.jobs.write().await;
        
        let instance = jobs.get_mut(label)
            .ok_or_else(|| NusaError::JobNotFound(label.to_string()))?;
        
        // Update state
        let previous_state = std::mem::replace(&mut instance.state, JobState::Stopping);
        
        // Get PID and handle
        let pid = instance.pid;
        let handle = instance.process_handle.take();
        
        drop(jobs); // Release lock
        
        // Send SIGTERM if running
        if let Some(pid) = pid {
            if let Err(e) = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGTERM
            ) {
                warn!("Failed to send SIGTERM to job '{}': {}", label, e);
            }
            
            // Wait for process to terminate with timeout
            if let Some(handle) = handle {
                tokio::select! {
                    _ = handle => {
                        debug!("Process terminated gracefully");
                    }
                    _ = time::sleep(Duration::from_secs(10)) => {
                        // Force kill after timeout
                        warn!("Job '{}' did not terminate gracefully, sending SIGKILL", label);
                        let _ = nix::sys::signal::kill(
                            nix::unistd::Pid::from_raw(pid as i32),
                            nix::sys::signal::Signal::SIGKILL
                        );
                    }
                }
            }
        }
        
        // Update state to stopped
        let mut jobs = self.jobs.write().await;
        let instance = jobs.get_mut(label).unwrap();
        
        instance.state = JobState::Stopped;
        instance.pid = None;
        instance.start_time = None;
        instance.process_handle = None;
        
        self.event_dispatcher.send(JobEvent::JobStopped(
            label.to_string(),
            previous_state
        )).await?;
        
        info!("Job stopped successfully");
        Ok(())
    }
    
    /// Restart a job
    pub async fn restart_job(&self, label: &str) -> Result<()> {
        self.stop_job(label).await?;
        time::sleep(Duration::from_millis(100)).await; // Brief pause
        self.start_job(label).await
    }
    
    /// Get job status
    pub async fn get_job_status(&self, label: &str) -> Option<JobStatus> {
        let jobs = self.jobs.read().await;
        jobs.get(label).map(|instance| {
            let uptime = instance.start_time.map(|t| t.elapsed());
            
            JobStatus {
                label: label.to_string(),
                state: instance.state.clone(),
                pid: instance.pid,
                restart_count: instance.restart_count,
                uptime,
                exit_code: instance.last_exit_code,
                exit_signal: instance.last_exit_signal,
                config: instance.config.clone(),
            }
        })
    }
    
    /// List all jobs
    pub async fn list_jobs(&self) -> Vec<JobStatus> {
        let jobs = self.jobs.read().await;
        jobs.iter()
            .map(|(label, instance)| {
                let uptime = instance.start_time.map(|t| t.elapsed());
                
                JobStatus {
                    label: label.clone(),
                    state: instance.state.clone(),
                    pid: instance.pid,
                    restart_count: instance.restart_count,
                    uptime,
                    exit_code: instance.last_exit_code,
                    exit_signal: instance.last_exit_signal,
                    config: instance.config.clone(),
                }
            })
            .collect()
    }
    
    /// Handle process exit
    pub async fn handle_process_exit(
        &self,
        label: String,
        exit_code: i32,
        signal: Option<i32>,
        restart_needed: bool,
    ) -> Result<()> {
        debug!("Handling process exit for job: {}", label);
        
        let mut jobs = self.jobs.write().await;
        
        let instance = jobs.get_mut(&label)
            .ok_or_else(|| NusaError::JobNotFound(label.clone()))?;
        
        // Update exit information
        instance.last_exit_code = Some(exit_code);
        instance.last_exit_signal = signal;
        instance.pid = None;
        instance.process_handle = None;
        
        // Determine next state
        if restart_needed {
            instance.state = JobState::Restarting;
            instance.restart_count += 1;
            
            // Check restart limits
            if instance.config.supervision.max_restarts > 0 &&
               instance.restart_count >= instance.config.supervision.max_restarts {
                instance.state = JobState::Failed(format!(
                    "Exceeded max restarts ({})",
                    instance.config.supervision.max_restarts
                ));
                self.event_dispatcher.send(JobEvent::JobFailed(
                    label.clone(),
                    instance.state.clone(),
                )).await?;
            } else {
                // Schedule restart with backoff
                let backoff_duration = self.calculate_backoff_duration(instance);
                instance.backoff_until = Some(Instant::now() + backoff_duration);
                instance.state = JobState::Backoff;
                
                // Send restart request
                self.restart_tx.send(RestartRequest {
                    label: label.clone(),
                    delay: backoff_duration,
                }).await
                .map_err(|e| NusaError::System(format!("Failed to schedule restart: {}", e)))?;
                
                self.event_dispatcher.send(JobEvent::JobRestartScheduled(
                    label.clone(),
                    backoff_duration,
                    instance.restart_count,
                )).await?;
            }
        } else {
            instance.state = JobState::Stopped;
            self.event_dispatcher.send(JobEvent::JobExited(
                label.clone(),
                exit_code,
                signal,
                instance.restart_count,
            )).await?;
        }
        
        Ok(())
    }
    
    /// Calculate backoff duration for restarts
    fn calculate_backoff_duration(&self, instance: &JobInstance) -> Duration {
        let base_delay = instance.config.supervision.restart_delay_sec;
        let multiplier = 2u64.pow(instance.restart_count.min(5)); // Cap exponential growth
        
        Duration::from_secs(base_delay * multiplier)
    }
}

impl Clone for JobManager {
    fn clone(&self) -> Self {
        Self {
            jobs: Arc::clone(&self.jobs),
            event_dispatcher: self.event_dispatcher.clone(),
            spawner: ProcessSpawner::new(self.event_dispatcher.clone()),
            restart_tx: self.restart_tx.clone(),
        }
    }
}

async fn handle_restart_request(
    request: RestartRequest,
    jobs: Arc<RwLock<HashMap<String, JobInstance>>>,
    event_dispatcher: EventDispatcher,
) {
    // Wait for the delay
    time::sleep(request.delay).await;
    
    let mut jobs = jobs.write().await;
    
    if let Some(instance) = jobs.get_mut(&request.label) {
        // Check if still in backoff/restarting state
        if matches!(instance.state, JobState::Backoff | JobState::Restarting) {
            // Reset state to stopped so it can be started again
            instance.state = JobState::Stopped;
            drop(jobs); // Release lock
            
            // Note: Actual restart will be triggered by external logic
            // This is just the scheduler
            let _ = event_dispatcher.send(JobEvent::JobReadyForRestart(request.label)).await;
        }
    }
}

#[derive(Debug)]
pub struct JobStatus {
    pub label: String,
    pub state: JobState,
    pub pid: Option<u32>,
    pub restart_count: u32,
    pub uptime: Option<Duration>,
    pub exit_code: Option<i32>,
    pub exit_signal: Option<i32>,
    pub config: JobConfig,
}

#[derive(Debug)]
pub enum JobEvent {
    JobLoaded(String),
    JobStarted(String, u32, Instant),
    JobStopped(String, JobState),
    JobExited(String, i32, Option<i32>, u32),
    JobFailed(String, JobState),
    JobRestartScheduled(String, Duration, u32),
    JobReadyForRestart(String),
}

#[derive(Debug)]
struct RestartRequest {
    label: String,
    delay: Duration,
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
            JobState::Backoff => write!(f, "backoff"),
        }
    }
}