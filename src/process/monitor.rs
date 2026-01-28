use tokio::process::Child;
use tokio::time::{timeout, Duration};
use tracing::{debug, warn, info};
use crate::event::dispatcher::EventDispatcher;
use crate::job::config::JobConfig;

pub struct ProcessMonitor;

impl ProcessMonitor {
    /// Monitor a process with timeout
    pub async fn monitor_with_timeout(
        mut child: Child,
        job_label: String,
        config: JobConfig,
        event_dispatcher: EventDispatcher,
        timeout_secs: u64,
    ) {
        let timeout_duration = Duration::from_secs(timeout_secs);
        
        match timeout(timeout_duration, child.wait()).await {
            Ok(Ok(status)) => {
                let exit_code = status.code().unwrap_or(-1);
                let signal = status.signal();
                
                info!(
                    "Job '{}' exited with code {} (signal: {:?})",
                    job_label, exit_code, signal
                );
                
                // Send event
                let _ = event_dispatcher.send(crate::job::manager::JobEvent::JobExited(
                    job_label,
                    exit_code,
                    signal,
                    0,
                )).await;
            }
            Ok(Err(e)) => {
                warn!("Error waiting for process: {}", e);
                
                let _ = event_dispatcher.send(crate::job::manager::JobEvent::JobFailed(
                    job_label,
                    crate::job::manager::JobState::Failed(format!("Wait error: {}", e)),
                )).await;
            }
            Err(_) => {
                warn!("Process monitor timeout after {} seconds", timeout_secs);
                
                // Try to kill the process
                if let Some(pid) = child.id() {
                    let _ = nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(pid as i32),
                        nix::sys::signal::Signal::SIGKILL
                    );
                }
                
                // Note: The JobExited event will be sent when the process actually exits
            }
        }
    }
    
    /// Check if a process is still running
    pub fn is_process_running(pid: u32) -> bool {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        
        kill(Pid::from_raw(pid as i32), None).is_ok()
    }
}