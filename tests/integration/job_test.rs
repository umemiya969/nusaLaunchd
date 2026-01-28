use nusalaunchd::job::{JobConfig, JobManager, RestartPolicy};
use nusalaunchd::job::config::{ProgramConfig, SupervisionConfig};
use std::path::PathBuf;
use tempfile::TempDir;
use tokio;
use std::time::Duration;

#[tokio::test]
async fn test_job_lifecycle() {
    // Create a temporary directory for test
    let temp_dir = TempDir::new().unwrap();
    
    // Create job manager
    let (manager, mut event_rx) = JobManager::new().await.unwrap();
    
    // Create a simple job configuration
    let config = JobConfig {
        label: "test-job".to_string(),
        description: Some("Test job".to_string()),
        program: ProgramConfig {
            path: PathBuf::from("/bin/sleep"),
            arguments: vec!["5".to_string()], // Sleep for 5 seconds
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
    
    // Test: Load job
    manager.load_job(config).await.expect("Failed to load job");
    
    // Verify job loaded event
    let event = event_rx.recv().await.unwrap();
    assert!(matches!(event, nusalaunchd::job::manager::JobEvent::JobLoaded(label) 
        if label == "test-job"));
    
    // Test: Get job status
    let status = manager.get_job_status("test-job").await;
    assert!(status.is_some());
    let status = status.unwrap();
    assert_eq!(status.label, "test-job");
    
    // Test: List jobs
    let jobs = manager.list_jobs().await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].label, "test-job");
}

#[tokio::test]
async fn test_job_restart_policy() {
    let (manager, _event_rx) = JobManager::new().await.unwrap();
    
    // Create job with restart policy
    let config = JobConfig {
        label: "restart-job".to_string(),
        description: None,
        program: ProgramConfig {
            path: PathBuf::from("/bin/true"),
            arguments: vec![],
        },
        supervision: SupervisionConfig {
            keep_alive: true,
            restart_policy: RestartPolicy::OnFailure,
            restart_delay_sec: 1,
            max_restarts: 3,
        },
        environment: vec![],
        working_directory: None,
    };
    
    manager.load_job(config).await.expect("Failed to load job");
    
    // Test restart policy evaluation
    use nusalaunchd::job::supervisor::JobSupervisor;
    let supervisor = JobSupervisor::new();
    
    // Test OnFailure policy
    let should_restart = supervisor.should_restart(
        &config.supervision,
        1, // non-zero exit code
        None,
        0,
    );
    assert!(should_restart);
    
    // Test Never policy
    let mut never_config = config.supervision.clone();
    never_config.restart_policy = RestartPolicy::Never;
    let should_restart = supervisor.should_restart(
        &never_config,
        1,
        None,
        0,
    );
    assert!(!should_restart);
    
    // Test max restarts limit
    let should_restart = supervisor.should_restart(
        &config.supervision,
        1,
        None,
        3, // At max restarts
    );
    assert!(!should_restart); // Should not restart beyond max
}

#[tokio::test]
async fn test_backoff_calculation() {
    use nusalaunchd::job::supervisor::JobSupervisor;
    let supervisor = JobSupervisor::new();
    
    let config = SupervisionConfig {
        keep_alive: true,
        restart_policy: RestartPolicy::Always,
        restart_delay_sec: 2,
        max_restarts: 5,
    };
    
    // Test exponential backoff
    let backoff1 = supervisor.calculate_backoff(&config, 0);
    assert_eq!(backoff1.as_secs(), 2); // 2 * 2^0 = 2
    
    let backoff2 = supervisor.calculate_backoff(&config, 1);
    assert_eq!(backoff2.as_secs(), 4); // 2 * 2^1 = 4
    
    let backoff3 = supervisor.calculate_backoff(&config, 2);
    assert_eq!(backoff3.as_secs(), 8); // 2 * 2^2 = 8
    
    // Test cap at 300 seconds (5 minutes)
    let backoff_large = supervisor.calculate_backoff(&config, 10);
    assert!(backoff_large.as_secs() <= 300);
}