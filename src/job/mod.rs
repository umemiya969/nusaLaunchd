//! Job management module for NusaLaunchd

pub mod config;
pub mod manager;

// Re-export commonly used types
pub use config::{JobConfig, ProgramConfig, SupervisionConfig, RestartPolicy};
pub use manager::{JobManager, JobState, JobEvent, JobStatus};