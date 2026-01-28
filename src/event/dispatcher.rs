use tokio::sync::mpsc;
use tracing::{info, warn, debug, instrument};

use crate::job::manager::JobEvent;
use crate::util::error::{NusaError, Result};

#[derive(Clone)]
pub struct EventDispatcher {
    tx: mpsc::Sender<JobEvent>,
}

impl EventDispatcher {
    pub fn new(tx: mpsc::Sender<JobEvent>) -> Self {
        Self { tx }
    }
    
    /// Send a job event
    #[instrument(skip(self), fields(event = ?event))]
    pub async fn send(&self, event: JobEvent) -> Result<()> {
        debug!("Dispatching event");
        
        self.tx.send(event).await
            .map_err(|e| NusaError::System(format!("Failed to send event: {}", e)))?;
        
        Ok(())
    }
    
    /// Process events from a receiver
    pub async fn process_events(mut rx: mpsc::Receiver<JobEvent>) {
        info!("Starting event processor");
        
        while let Some(event) = rx.recv().await {
            match &event {
                JobEvent::JobLoaded(label) => {
                    info!("[EVENT] Job loaded: {}", label);
                }
                JobEvent::JobStarted(label, pid, _) => {
                    info!("[EVENT] Job started: {} [PID: {}]", label, pid);
                }
                JobEvent::JobStopped(label, previous_state) => {
                    info!("[EVENT] Job stopped: {} (was: {:?})", label, previous_state);
                }
                JobEvent::JobExited(label, code, signal, restart_count) => {
                    let signal_info = signal.map(|s| format!("signal {}", s))
                        .unwrap_or_else(|| "normally".to_string());
                    info!(
                        "[EVENT] Job exited: {} with code {}, {} (restarts: {})",
                        label, code, signal_info, restart_count
                    );
                }
                JobEvent::JobFailed(label, state) => {
                    warn!("[EVENT] Job failed: {} with state: {:?}", label, state);
                }
                JobEvent::JobRestartScheduled(label, delay, attempt) => {
                    info!(
                        "[EVENT] Job restart scheduled: {} in {:?} (attempt {})",
                        label, delay, attempt
                    );
                }
                JobEvent::JobReadyForRestart(label) => {
                    info!("[EVENT] Job ready for restart: {}", label);
                }
            }
            
            // TODO: Add hooks for external event listeners
            // TODO: Persist events to log file/database
        }
    }
}