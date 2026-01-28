use nusalaunchd::job::config::JobConfig;
use tempfile::NamedTempFile;

#[test]
fn test_config_parsing() {
    let toml_content = r#"
        label = "web-server"
        description = "Nginx web server"
        
        [program]
        path = "/usr/sbin/nginx"
        arguments = [")g", "daemon off;"]
        
        [supervision]
        keep_alive = true
        restart_policy = "on-failure"
        restart_delay_sec = 5
        max_restarts = 3
        
        [[environment]]
        key = "NGINX_ENV"
        value = "production"
        
        [[environment]]
        key = "RUST_LOG"
        value = "info"
        
        working_directory = "/var/www"
    "#;
    
    let mut file = NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut file, toml_content.as_bytes()).unwrap();
    
    let config = JobConfig::from_file(&file).await.unwrap();
    
    assert_eq!(config.label, "web-server");
    assert_eq!(config.description.unwrap(), "Nginx web server");
    assert_eq!(config.program.path, std::path::PathBuf::from("/usr/sbin/nginx"));
    assert_eq!(config.program.arguments, vec!["-g", "daemon off;"]);
    assert_eq!(config.supervision.keep_alive, true);
    assert_eq!(config.supervision.restart_delay_sec, 5);
    assert_eq!(config.supervision.max_restarts, 3);
    assert_eq!(config.environment.len(), 2);
    assert_eq!(config.working_directory.unwrap(), std::path::PathBuf::from("/var/www"));
}

#[test]
fn test_config_validation() {
    // Test empty label
    let toml_content = r#"
        label = ""
        [program]
        path = "/bin/true"
    "#;
    
    let mut file = NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut file, toml_content.as_bytes()).unwrap();
    
    let result = JobConfig::from_file(&file).await;
    assert!(result.is_err());
}

#[test]
fn test_environment_parsing() {
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
        
        [[environment]]
        key = "DEBUG"
        value = "1"
    "#;
    
    let mut file = NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut file, toml_content.as_bytes()).unwrap();
    
    let config = JobConfig::from_file(&file).await.unwrap();
    
    assert_eq!(config.environment.len(), 3);
    
    let env_vars = config.get_env_vars();
    assert_eq!(env_vars.len(), 3);
    
    // Check that environment variables are correctly converted
    let mut env_map = std::collections::HashMap::new();
    for (key, value) in env_vars {
        env_map.insert(key, value);
    }
    
    assert_eq!(env_map.get("HOME"), Some(&"/tmp/test".to_string()));
    assert_eq!(env_map.get("PATH"), Some(&"/usr/bin:/bin".to_string()));
    assert_eq!(env_map.get("DEBUG"), Some(&"1".to_string()));
}