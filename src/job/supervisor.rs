use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio::time;
use tracing::{info, warn, debug, instrument};

use crate::job::config::{SupervisionConfig, RestartPolicy};
use crate::util::error::{NusaError, Result};

pub struct JobSupervisor {
    restart_queue: Arc<Mutex<Vec<RestartJob>>>,
    backoff_tracker: Arc<RwLock<HashMap<String, BackoffInfo>>>,
}

impl JobSupervisor {
    pub fn new() -> Self {
        Self {
            restart_queue: Arc::new(Mutex::new(Vec::new())),
            backoff_tracker: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Check if a job should be restarted based on exit status
    pub fn should_restart(
        &self,
        config: &SupervisionConfig,
        exit_code: i32,
        signal: Option<i32>,
        restart_count: u32,
    ) -> bool {
        if !config.keep_alive {
            return false;
        }
        
        // Check restart limit
        if config.max_restarts > 0 && restart_count >= config.max_restarts {
            debug!("Max restarts ({}) exceeded", config.max_restarts);
            return false;
        }
        
        // Check restart policy
        match config.restart_policy {
            RestartPolicy::Always => true,
            RestartPolicy::Never => false,
            RestartPolicy::OnFailure => exit_code != 0,
            RestartPolicy::OnCrash => signal.is_some(),
        }
    }
    
    /// Calculate backoff duration for restart
    pub fn calculate_backoff(&self, config: &SupervisionConfig, restart_count: u32) -> Duration {
        let base_secs = config.restart_delay_sec;
        
        // Exponential backoff with cap
        let exponent = restart_count.min(6); // Cap at 2^6 = 64x base delay
        let multiplier = 2u64.pow(exponent);
        
        let backoff_secs = base_secs * multiplier;
        
        // Cap at 5 minutes max
        Duration::from_secs(backoff_secs.min(300))
    }
    
    /// Schedule a job for restart
    #[instrument(skip(self), fields(job = %label))]
    pub async fn schedule_restart(
        &self,
        label: String,
        config: SupervisionConfig,
        restart_count: u32,
    ) -> Result<Duration> {
        let backoff = self.calculate_backoff(&config, restart_count);
        
        let restart_job = RestartJob {
            label: label.clone(),
            scheduled_at: Instant::now() + backoff,
            attempt: restart_count + 1,
        };
        
        {
            let mut queue = self.restart_queue.lock().await;
            queue.push(restart_job);
            queue.sort_by_key(|j| j.scheduled_at); // Sort by earliest first
        }
        
        // Store backoff info
        {
            let mut tracker = self.backoff_tracker.write().await;
            tracker.insert(label.clone(), BackoffInfo {
                backoff_until: Instant::now() + backoff,
                attempt: restart_count + 1,
            });
        }
        
        info!(
            "Scheduled restart for job '{}' in {} seconds (attempt {})",
            label,
            backoff.as_secs(),
            restart_count + 1
        );
        
        Ok(backoff)
    }
    
    /// Get jobs ready for restart
    pub async fn get_ready_jobs(&self) -> Vec<String> {
        let now = Instant::now();
        let mut ready = Vec::new();
        
        let mut queue = self.restart_queue.lock().await;
        
        // Find all jobs whose scheduled time has passed
        let mut i = 0;
        while i < queue.len() {
            if queue[i].scheduled_at <= now {
                let job = queue.remove(i);
                ready.push(job.label);
                
                // Remove from backoff tracker
                let mut tracker = self.backoff_tracker.write().await;
                tracker.remove(&job.label);
            } else {
                i += 1;
            }
        }
        
        ready
    }
    
    /// Cancel scheduled restart for a job
    pub async fn cancel_restart(&self, label: &str) {
        {
            let mut queue = self.restart_queue.lock().await;
            queue.retain(|j| j.label != label);
        }
        
        {
            let mut tracker = self.backoff_tracker.write().await;
            tracker.remove(label);
        }
        
        debug!("Cancelled restart for job: {}", label);
    }
    
    /// Check if a job is in backoff period
    pub async fn is_in_backoff(&self, label: &str) -> Option<Duration> {
        let tracker = self.backoff_tracker.read().await;
        
        tracker.get(label).and_then(|info| {
            if info.backoff_until > Instant::now() {
                Some(info.backoff_until - Instant::now())
            } else {
                None
            }
        })
    }
    
    /// Start background task to process restart queue
    pub fn start_restart_processor(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(1));
            
            loop {
                interval.tick().await;
                
                // Get ready jobs
                let ready_jobs = self.get_ready_jobs().await;
                
                for label in ready_jobs {
                    info!("Job '{}' is ready for restart", label);
                    
                    // In a real implementation, this would send an event
                    // to the job manager to restart the job
                }
            }
        })
    }
}

#[derive(Debug, Clone)]
struct RestartJob {
    label: String,
    scheduled_at: Instant,
    attempt: u32,
}

#[derive(Debug)]
struct BackoffInfo {
    backoff_until: Instant,
    attempt: u32,
}