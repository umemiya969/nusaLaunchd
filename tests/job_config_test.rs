use nusalaunchd::job::config::JobConfig;
use tempfile::NamedTempFile;

#[test]
fn test_basic_config_parsing() {
    let toml_content = r#"
        label = "test-service"
        
        [program]
        path = "/bin/true"
        
        [supervision]
        keep_alive = true
        restart_policy = "on-failure"
    "#;
    
    let mut file = NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut file, toml_content.as_bytes()).unwrap();
    
    let config = JobConfig::from_file(file.path()).unwrap();
    
    assert_eq!(config.label, "test-service");
    assert_eq!(config.program.path, std::path::PathBuf::from("/bin/true"));
    assert_eq!(config.supervision.keep_alive, true);
}

#[test]
fn test_environment_vars() {
    let toml_content = r#"
        label = "env-test"
        
        [program]
        path = "/bin/bash"
        
        [[environment]]
        key = "HOME"
        value = "/tmp/test"
        
        [[environment]]
        key = "PATH"
        value = "/usr/bin:/bin"
    "#;
    
    let mut file = NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut file, toml_content.as_bytes()).unwrap();
    
    let config = JobConfig::from_file(file.path()).unwrap();
    
    assert_eq!(config.environment.len(), 2);
    assert_eq!(config.environment[0].key, "HOME");
    assert_eq!(config.environment[0].value, "/tmp/test");
}