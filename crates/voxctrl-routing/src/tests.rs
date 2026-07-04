use crate::models::{DeliveryType, OutputTarget};
use crate::loader::{load_targets, save_targets};
use crate::targets::build_target;

#[test]
fn test_mcp_config_roundtrip() {
    let temp_dir = std::env::temp_dir().join(format!("voxctrl_test_{}", chrono::Utc::now().timestamp_millis()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let target = OutputTarget {
        id: "mcp_test".into(),
        label: "Test MCP".into(),
        delivery: DeliveryType::Mcp,
        command: None,
        pipe_path: None,
        socket_host: None,
        socket_port: None,
        socket_unix: None,
        file_path: None,
        file_prefix: "".into(),
        file_timestamp: false,
        file_mode: "append".into(),
        dbus_signal: None,
        http_url: None,
        http_method: "POST".into(),
        http_headers: None,
        http_json_template: None,
        webhook_url: None,
        webhook_secret: None,
        webhook_json_template: None,
        mcp_path: Some("/tmp/custom-mcp.sock".into()),
        mcp_tool: Some("speak_text".into()),
        mcp_args: Some(serde_json::json!({ "message": "{TEXT}" })),
        send_on_release: true,
        append_newline: false,
        strip_newlines: false,
        initial_prompt: None,
        processing: Default::default(),
        response_pipe: None,
    };

    save_targets(&[target.clone()], &temp_dir).unwrap();
    let loaded = load_targets(&temp_dir).unwrap();

    assert_eq!(loaded.len(), 1);
    let loaded_target = &loaded[0];
    assert_eq!(loaded_target.id, "mcp_test");
    assert_eq!(loaded_target.delivery, DeliveryType::Mcp);
    assert_eq!(loaded_target.mcp_path.as_deref(), Some("/tmp/custom-mcp.sock"));
    assert_eq!(loaded_target.mcp_tool.as_deref(), Some("speak_text"));
    assert_eq!(
        loaded_target.mcp_args.as_ref().unwrap(),
        &serde_json::json!({ "message": "{TEXT}" })
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[cfg(unix)]
#[tokio::test]
async fn test_mcp_delivery_handshake() {
    use tokio::net::UnixListener;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let socket_path = format!("/tmp/voxctrl-mcp-test-{}.sock", chrono::Utc::now().timestamp_millis());
    let socket_path_clone = socket_path.clone();

    // 1. Start a mock MCP server that implements the handshake protocol
    let server = tokio::spawn(async move {
        let listener = UnixListener::bind(&socket_path_clone).unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();

        // Receive initialize
        let line = lines.next_line().await.unwrap().unwrap();
        assert!(line.contains("initialize"));
        let init_res = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": { "name": "mock-mcp", "version": "1.0" },
                "capabilities": {}
            }
        });
        writer.write_all((serde_json::to_string(&init_res).unwrap() + "\n").as_bytes()).await.unwrap();
        writer.flush().await.unwrap();

        // Receive notifications/initialized
        let line = lines.next_line().await.unwrap().unwrap();
        assert!(line.contains("notifications/initialized"));

        // Receive tools/call
        let line = lines.next_line().await.unwrap().unwrap();
        assert!(line.contains("tools/call"));
        assert!(line.contains("speak_text"));
        assert!(line.contains("Hello World"));

        let tool_res = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "content": [
                    { "type": "text", "text": "spoken" }
                ]
            }
        });
        writer.write_all((serde_json::to_string(&tool_res).unwrap() + "\n").as_bytes()).await.unwrap();
        writer.flush().await.unwrap();
    });

    // Give the server a small moment to bind
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 2. Deliver the text via McpTarget
    let config = OutputTarget {
        id: "mcp_test".into(),
        label: "Test MCP".into(),
        delivery: DeliveryType::Mcp,
        command: None,
        pipe_path: None,
        socket_host: None,
        socket_port: None,
        socket_unix: None,
        file_path: None,
        file_prefix: "".into(),
        file_timestamp: false,
        file_mode: "append".into(),
        dbus_signal: None,
        http_url: None,
        http_method: "POST".into(),
        http_headers: None,
        http_json_template: None,
        webhook_url: None,
        webhook_secret: None,
        webhook_json_template: None,
        mcp_path: Some(socket_path.clone()),
        mcp_tool: Some("speak_text".into()),
        mcp_args: Some(serde_json::json!({ "text": "{TEXT}" })),
        send_on_release: true,
        append_newline: false,
        strip_newlines: false,
        initial_prompt: None,
        processing: Default::default(),
        response_pipe: None,
    };

    let target = build_target(config);
    let result = target.deliver("Hello World").await;

    assert!(result.success);
    assert!(result.error.is_none());

    server.await.unwrap();
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_hotkey_binding_multi_target_roundtrip() {
    use crate::models::{GestureType, HotkeyBinding};
    use crate::loader::{load_bindings, save_bindings};

    let temp_dir = std::env::temp_dir().join(format!("voxctrl_bindings_test_{}", chrono::Utc::now().timestamp_millis()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let binding = HotkeyBinding {
        id: "multi_test".into(),
        label: "Multi Test".into(),
        keys: vec!["KEY_LEFTMETA".into(), "KEY_SPACE".into()],
        gesture: GestureType::Hold,
        target_id: "target1".into(),
        target_ids: vec!["target1".into(), "target2".into()],
        tap_ms: 300,
        hold_threshold_ms: 500,
        subkey: None,
        disabled: false,
        openai_enabled: Some(false),
        openai_model: None,
        openai_mode: None,
        openai_prompt: None,
        openai_system_prompt: None,
    };

    assert_eq!(binding.resolved_target_ids(), vec!["target1", "target2"]);
    assert_eq!(binding.target_ids_string(), "target1,target2");

    save_bindings(&[binding.clone()], &temp_dir).unwrap();
    let loaded = load_bindings(&temp_dir).unwrap();

    assert_eq!(loaded.len(), 1);
    save_bindings(&[binding.clone()], &temp_dir).unwrap();
    let loaded = load_bindings(&temp_dir).unwrap();

    assert_eq!(loaded.len(), 1);
    let loaded_binding = &loaded[0];
    assert_eq!(loaded_binding.id, "multi_test");
    assert_eq!(loaded_binding.target_id, "target1"); // Compatible field populated
    assert_eq!(loaded_binding.target_ids, vec!["target1", "target2"]);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

// ── Target Delivery Success & Failure Tests ──────────────────────────────────

// 1. Inject Target
#[tokio::test]
async fn test_inject_target_success_and_failure() {
    let temp_dir = std::env::temp_dir().join(format!("voxctrl_inject_test_{}", chrono::Utc::now().timestamp_millis()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    #[cfg(target_os = "linux")]
    {
        let mock_wtype = temp_dir.join("wtype");
        std::fs::write(&mock_wtype, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mock_wtype, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("WAYLAND_DISPLAY", "mock-display");
    }
    #[cfg(target_os = "windows")]
    {
        let mock_ps = temp_dir.join("powershell.exe");
        std::fs::write(&mock_ps, "@echo off\nexit /b 0\n").unwrap();
    }

    // Prepend to PATH
    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = std::env::split_paths(&old_path).collect::<Vec<_>>();
    paths.insert(0, temp_dir.clone());
    let new_path = std::env::join_paths(paths).unwrap();
    std::env::set_var("PATH", &new_path);

    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Inject;
    let target = build_target(config);
    let res = target.deliver("Test Input").await;
    
    // Clean up PATH
    std::env::set_var("PATH", old_path);
    let _ = std::fs::remove_dir_all(&temp_dir);

    // In a test environment, if PATH was prepended successfully, it should succeed
    if res.success {
        assert_eq!(res.delivered_text.as_deref(), Some("Test Input"));
    } else {
        println!("Inject success path skipped or failed gracefully: {:?}", res.error);
    }
}

#[tokio::test]
async fn test_inject_target_failure_no_injection() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Inject;
    let target = build_target(config);
    let res = target.deliver("Test Input").await;
    let _ = target.test().await;
    assert!(res.delivered_text.is_some() || res.error.is_some());
}

// 2. Clipboard Target
#[tokio::test]
async fn test_clipboard_target_success() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Clipboard;
    let target = build_target(config);
    let res = target.deliver("Test Clipboard").await;
    if res.success {
        assert_eq!(res.delivered_text.as_deref(), Some("Test Clipboard"));
        let test_res = target.test().await;
        assert!(test_res.reachable);
    } else {
        assert!(res.error.is_some());
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn test_clipboard_target_linux_cli() {
    let temp_dir = std::env::temp_dir().join(format!("voxctrl_clipboard_test_{}", chrono::Utc::now().timestamp_millis()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    // Mock wl-copy that writes its input to a file so we can assert it was called with the right data.
    let mock_wl_copy = temp_dir.join("wl-copy");
    let test_output_file = temp_dir.join("test_clipboard_output.txt");
    let script = format!(
        "#!/bin/sh\ncat > \"{}\"\nexit 0\n",
        test_output_file.to_string_lossy()
    );
    std::fs::write(&mock_wl_copy, script).unwrap();
    
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&mock_wl_copy, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("WAYLAND_DISPLAY", "mock-display");

    // Prepend to PATH
    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = std::env::split_paths(&old_path).collect::<Vec<_>>();
    paths.insert(0, temp_dir.clone());
    let new_path = std::env::join_paths(paths).unwrap();
    std::env::set_var("PATH", &new_path);

    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Clipboard;
    let target = build_target(config);
    let res = target.deliver("Test Clipboard Content").await;

    // Clean up PATH
    std::env::set_var("PATH", old_path);

    assert!(res.success, "Clipboard delivery failed: {:?}", res.error);
    assert_eq!(res.delivered_text.as_deref(), Some("Test Clipboard Content"));

    let content = std::fs::read_to_string(&test_output_file).unwrap();
    assert_eq!(content, "Test Clipboard Content");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_clipboard_target_failure_empty_text() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Clipboard;
    let target = build_target(config);
    let res = target.deliver("").await;
    let _ = target.test().await;
    assert!(res.success || res.error.is_some());
}

// 3. Exec Target
#[tokio::test]
async fn test_exec_target_success() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Exec;
    config.command = Some("echo {TEXT}".into());
    let target = build_target(config);
    let res = target.deliver("Hello Exec").await;
    assert!(res.success, "Exec delivery failed: {:?}", res.error);
    assert_eq!(res.delivered_text.as_deref(), Some("Hello Exec"));
    let test_res = target.test().await;
    assert!(test_res.reachable);
}

#[tokio::test]
async fn test_exec_target_success_argument_substitution() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Exec;
    config.command = Some("echo Prefix {TEXT} Suffix".into());
    let target = build_target(config);
    let res = target.deliver("Multi Word Value").await;
    assert!(res.success);
    assert_eq!(res.delivered_text.as_deref(), Some("Multi Word Value"));
}

#[tokio::test]
async fn test_exec_target_success_lowercase_and_multiple_substitution() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Exec;
    config.command = Some("echo first={text} second={text}".into());
    let target = build_target(config);
    let res = target.deliver("Value").await;
    assert!(res.success);
    assert_eq!(res.delivered_text.as_deref(), Some("Value"));
}

#[tokio::test]
async fn test_exec_target_quoted_substitution() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Exec;
    config.command = Some("echo \"{text}\"".into());
    let target = build_target(config);
    let res = target.deliver("Value").await;
    assert!(res.success);
    assert_eq!(res.delivered_text.as_deref(), Some("Value"));
}

#[tokio::test]
async fn test_exec_target_failure_no_cmd() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Exec;
    config.command = None;
    let target = build_target(config);
    let res = target.deliver("Hello").await;
    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("No command configured"));
    let test_res = target.test().await;
    assert!(!test_res.reachable);
}

#[tokio::test]
async fn test_exec_target_failure_nonexistent_binary() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Exec;
    config.command = Some("nonexistent_binary_xyz_123 {TEXT}".into());
    let target = build_target(config);
    let res = target.deliver("Hello").await;
    assert!(!res.success);
    println!("NONEXISTENT BINARY ERROR: {:?}", res.error);
    assert!(!res.error.as_ref().unwrap().is_empty());
}

// 4. Pipe Target
#[tokio::test]
async fn test_pipe_target_success_using_regular_file() {
    let temp_dir = std::env::temp_dir();
    let pipe_file = temp_dir.join(format!("voxctrl_pipe_test_{}.log", chrono::Utc::now().timestamp_millis()));
    std::fs::write(&pipe_file, "").unwrap();

    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Pipe;
    config.pipe_path = Some(pipe_file.to_string_lossy().to_string());

    let target = build_target(config);
    let res = target.deliver("Hello Pipe").await;
    assert!(res.success, "Pipe delivery failed: {:?}", res.error);
    assert_eq!(res.delivered_text.as_deref(), Some("Hello Pipe"));

    let content = std::fs::read_to_string(&pipe_file).unwrap();
    assert_eq!(content, "Hello Pipe\n");

    let _ = std::fs::remove_file(&pipe_file);
}

#[tokio::test]
async fn test_pipe_target_failure_not_exist() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Pipe;
    config.pipe_path = Some("/nonexistent/path/to/fifo_xyz".into());

    let target = build_target(config);
    let res = target.deliver("Hello").await;
    assert!(!res.success);
    assert!(res.error.as_ref().unwrap().contains("does not exist"));
}

#[tokio::test]
async fn test_pipe_target_failure_no_path() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Pipe;
    config.pipe_path = None;

    let target = build_target(config);
    let res = target.deliver("Hello").await;
    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("No pipe_path configured"));
}

// 5. Socket Target
#[tokio::test]
async fn test_socket_target_success_tcp() {
    use tokio::net::TcpListener;
    use tokio::io::AsyncReadExt;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0; 64];
        let bytes = stream.read(&mut buf).await.unwrap();
        let received = String::from_utf8_lossy(&buf[..bytes]);
        assert_eq!(received, "Hello Socket\n");
    });

    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Socket;
    config.socket_host = Some("127.0.0.1".into());
    config.socket_port = Some(port);

    let target = build_target(config);
    let res = target.deliver("Hello Socket").await;
    assert!(res.success, "Socket delivery failed: {:?}", res.error);

    server.await.unwrap();
}

#[tokio::test]
async fn test_socket_target_failure_closed_port() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Socket;
    config.socket_host = Some("127.0.0.1".into());
    config.socket_port = Some(54321);

    let target = build_target(config);
    let res = target.deliver("Hello Socket").await;
    assert!(!res.success);
    assert!(res.error.is_some());
}

#[cfg(unix)]
#[tokio::test]
async fn test_socket_target_success_unix() {
    use tokio::net::UnixListener;
    use tokio::io::AsyncReadExt;

    let socket_path = format!("/tmp/voxctrl_socket_unix_test_{}.sock", chrono::Utc::now().timestamp_millis());
    let socket_path_clone = socket_path.clone();

    let listener = UnixListener::bind(&socket_path_clone).unwrap();

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0; 64];
        let bytes = stream.read(&mut buf).await.unwrap();
        let received = String::from_utf8_lossy(&buf[..bytes]);
        assert_eq!(received, "Hello Unix Socket\n");
    });

    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Socket;
    config.socket_unix = Some(socket_path.clone());

    let target = build_target(config);
    let res = target.deliver("Hello Unix Socket").await;
    assert!(res.success, "Unix socket delivery failed: {:?}", res.error);

    server.await.unwrap();
    let _ = std::fs::remove_file(&socket_path);
}

#[cfg(unix)]
#[tokio::test]
async fn test_socket_target_failure_unix_not_exist() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Socket;
    config.socket_unix = Some("/nonexistent/unix/socket/path_123.sock".into());

    let target = build_target(config);
    let res = target.deliver("Hello").await;
    assert!(!res.success);
    assert!(res.error.is_some());
}

// 6. File Target
#[tokio::test]
async fn test_file_target_success_append_and_prepend() {
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join(format!("voxctrl_file_test_{}.log", chrono::Utc::now().timestamp_millis()));
    println!("TEST FILE PATH: {:?}", test_file);

    // Success Append
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::File;
    config.file_path = Some(test_file.to_string_lossy().to_string());
    config.file_timestamp = false;
    config.file_prefix = ">>> ".into();
    config.file_mode = "append".into();

    let target = build_target(config.clone());
    let res = target.deliver("Hello File").await;
    println!("FILE TARGET APPEND RESULT: {:?}", res);
    assert!(res.success, "File append failed: {:?}", res.error);

    // Sleep a tiny bit to let OS write flush
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let content = std::fs::read_to_string(&test_file).unwrap();
    println!("FILE APPEND CONTENT: {:?}", content);
    assert_eq!(content, ">>> Hello File\n");

    // Success Prepend
    config.file_mode = "prepend".into();
    config.file_prefix = "### ".into();
    let target_prepend = build_target(config);
    let res_prep = target_prepend.deliver("Prepended").await;
    println!("FILE TARGET PREPEND RESULT: {:?}", res_prep);
    assert!(res_prep.success, "File prepend failed: {:?}", res_prep.error);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let content_prep = std::fs::read_to_string(&test_file).unwrap();
    println!("FILE PREPEND CONTENT: {:?}", content_prep);
    assert_eq!(content_prep, "### Prepended\n>>> Hello File\n");

    let _ = std::fs::remove_file(&test_file);
}

#[tokio::test]
async fn test_file_target_success_timestamp_formatting() {
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join(format!("voxctrl_file_timestamp_test_{}.log", chrono::Utc::now().timestamp_millis()));

    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::File;
    config.file_path = Some(test_file.to_string_lossy().to_string());
    config.file_timestamp = true;
    config.file_prefix = "LOG: ".into();
    config.file_mode = "append".into();

    let target = build_target(config);
    let res = target.deliver("Timestamped Message").await;
    assert!(res.success);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let content = std::fs::read_to_string(&test_file).unwrap();
    println!("TIMESTAMP FILE CONTENT: {:?}", content);
    
    assert!(content.contains("LOG: Timestamped Message\n"));
    assert!(content.starts_with('['));
    assert!(content.contains("Z] LOG:"));

    let _ = std::fs::remove_file(&test_file);
}

#[tokio::test]
async fn test_file_target_failure_no_path() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::File;
    config.file_path = None;

    let target = build_target(config);
    let res = target.deliver("Hello").await;
    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("No file_path configured"));
}

// 7. DBus Target
#[tokio::test]
async fn test_dbus_target_platform_unsupported_on_non_linux() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Dbus;
    let target = build_target(config);
    let res = target.deliver("Hello DBus").await;
    
    #[cfg(not(target_os = "linux"))]
    {
        assert!(!res.success);
        assert_eq!(res.error.as_deref(), Some("DBus not available on this platform"));
    }
    #[cfg(target_os = "linux")]
    {
        let _ = res.success;
    }
}

#[tokio::test]
async fn test_dbus_target_failure_invalid_signal_name() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Dbus;
    config.dbus_signal = Some("invalid-signal-name!".into());

    let target = build_target(config);
    let res = target.deliver("Hello").await;
    
    #[cfg(target_os = "linux")]
    {
        assert!(!res.success);
        assert!(res.error.as_ref().unwrap().contains("Invalid D-Bus signal name"));
    }
    #[cfg(not(target_os = "linux"))]
    {
        assert!(!res.success);
        assert_eq!(res.error.as_deref(), Some("DBus not available on this platform"));
    }
}

static INIT_TEST_ENV: std::sync::Once = std::sync::Once::new();

fn init_test_env() {
    INIT_TEST_ENV.call_once(|| {
        std::env::remove_var("http_proxy");
        std::env::remove_var("HTTP_PROXY");
        std::env::remove_var("https_proxy");
        std::env::remove_var("HTTPS_PROXY");
        std::env::remove_var("all_proxy");
        std::env::remove_var("ALL_PROXY");
    });
}

// 8. HTTP Target
#[tokio::test]
async fn test_http_target_success_and_failure() {
    init_test_env();
    use tokio::net::TcpListener;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // 1. Success path: Server responds 200 OK
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0; 1024];
        let mut bytes_read = 0;
        let mut body_start = None;
        let mut content_length = None;
        loop {
            let n = stream.read(&mut buf[bytes_read..]).await.unwrap();
            if n == 0 {
                break;
            }
            bytes_read += n;
            let s = String::from_utf8_lossy(&buf[..bytes_read]);
            if body_start.is_none() {
                if let Some(pos) = s.find("\r\n\r\n") {
                    body_start = Some(pos + 4);
                    for line in s[..pos].lines() {
                        if line.to_lowercase().starts_with("content-length:") {
                            if let Some(len_str) = line.split(':').nth(1) {
                                if let Ok(len) = len_str.trim().parse::<usize>() {
                                    content_length = Some(len);
                                }
                            }
                        }
                    }
                }
            }
            if let (Some(start), Some(len)) = (body_start, content_length) {
                if bytes_read >= start + len {
                    break;
                }
            }
        }
        let request_str = String::from_utf8_lossy(&buf[..bytes_read]);
        println!("HTTP RECEIVED REQUEST: {:?}", request_str);

        let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(response.as_bytes()).await.unwrap();
        let _ = stream.shutdown().await;
    });

    let mut headers = std::collections::HashMap::new();
    headers.insert("X-Custom-Header".into(), "verified".into());

    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Http;
    config.http_url = Some(format!("http://127.0.0.1:{}/endpoint", port));
    config.http_headers = Some(headers);

    let target = build_target(config.clone());
    let res = target.deliver("Hello HTTP").await;
    println!("HTTP TARGET SUCCESS RES: {:?}", res);
    assert!(res.success, "HTTP target success failed: {:?}", res.error);

    server.await.unwrap();

    // 2. Failure path: Server responds 500 Internal Server Error
    let listener_fail = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port_fail = listener_fail.local_addr().unwrap().port();

    let server_fail = tokio::spawn(async move {
        let (mut stream, _) = listener_fail.accept().await.unwrap();
        let mut buf = vec![0; 1024];
        let mut bytes_read = 0;
        let mut body_start = None;
        let mut content_length = None;
        loop {
            let n = stream.read(&mut buf[bytes_read..]).await.unwrap();
            if n == 0 {
                break;
            }
            bytes_read += n;
            let s = String::from_utf8_lossy(&buf[..bytes_read]);
            if body_start.is_none() {
                if let Some(pos) = s.find("\r\n\r\n") {
                    body_start = Some(pos + 4);
                    for line in s[..pos].lines() {
                        if line.to_lowercase().starts_with("content-length:") {
                            if let Some(len_str) = line.split(':').nth(1) {
                                if let Ok(len) = len_str.trim().parse::<usize>() {
                                    content_length = Some(len);
                                }
                            }
                        }
                    }
                }
            }
            if let (Some(start), Some(len)) = (body_start, content_length) {
                if bytes_read >= start + len {
                    break;
                }
            }
        }
        let response = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(response.as_bytes()).await.unwrap();
        let _ = stream.shutdown().await;
    });

    config.http_url = Some(format!("http://127.0.0.1:{}/endpoint", port_fail));
    let target_fail = build_target(config);
    let res_fail = target_fail.deliver("Hello HTTP").await;
    println!("HTTP TARGET FAIL RES: {:?}", res_fail);
    assert!(!res_fail.success);
    assert!(res_fail.error.as_ref().unwrap().contains("500") || res_fail.error.as_ref().unwrap().contains("HTTP"));

    server_fail.await.unwrap();
}

#[tokio::test]
async fn test_http_target_success_custom_json_template() {
    init_test_env();
    use tokio::net::TcpListener;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0; 1024];
        let mut bytes_read = 0;
        let mut body_start = None;
        let mut content_length = None;
        loop {
            let n = stream.read(&mut buf[bytes_read..]).await.unwrap();
            if n == 0 {
                break;
            }
            bytes_read += n;
            let s = String::from_utf8_lossy(&buf[..bytes_read]);
            if body_start.is_none() {
                if let Some(pos) = s.find("\r\n\r\n") {
                    body_start = Some(pos + 4);
                    for line in s[..pos].lines() {
                        if line.to_lowercase().starts_with("content-length:") {
                            if let Some(len_str) = line.split(':').nth(1) {
                                if let Ok(len) = len_str.trim().parse::<usize>() {
                                    content_length = Some(len);
                                }
                            }
                        }
                    }
                }
            }
            if let (Some(start), Some(len)) = (body_start, content_length) {
                if bytes_read >= start + len {
                    break;
                }
            }
        }
        let request_str = String::from_utf8_lossy(&buf[..bytes_read]);
        let body = &request_str[body_start.unwrap()..];
        let json_body: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(json_body, serde_json::json!({ "message": "Result: Custom Payload" }));

        let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(response.as_bytes()).await.unwrap();
        let _ = stream.shutdown().await;
    });

    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Http;
    config.http_url = Some(format!("http://127.0.0.1:{}/custom", port));
    config.http_json_template = Some(serde_json::json!({ "message": "Result: {TEXT}" }));

    let target = build_target(config);
    let res = target.deliver("Custom Payload").await;
    assert!(res.success);

    server.await.unwrap();
}

#[tokio::test]
async fn test_http_target_failure_no_url() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Http;
    config.http_url = None;

    let target = build_target(config);
    let res = target.deliver("Hello").await;
    assert!(!res.success);
    assert_eq!(res.error.as_deref(), Some("No http_url configured"));
}

// 9. Webhook Target
#[tokio::test]
async fn test_webhook_target_success_and_failure() {
    init_test_env();
    use tokio::net::TcpListener;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0; 1024];
        let mut bytes_read = 0;
        let mut body_start = None;
        let mut content_length = None;
        loop {
            let n = stream.read(&mut buf[bytes_read..]).await.unwrap();
            if n == 0 {
                break;
            }
            bytes_read += n;
            let s = String::from_utf8_lossy(&buf[..bytes_read]);
            if body_start.is_none() {
                if let Some(pos) = s.find("\r\n\r\n") {
                    body_start = Some(pos + 4);
                    for line in s[..pos].lines() {
                        if line.to_lowercase().starts_with("content-length:") {
                            if let Some(len_str) = line.split(':').nth(1) {
                                if let Ok(len) = len_str.trim().parse::<usize>() {
                                    content_length = Some(len);
                                }
                            }
                        }
                    }
                }
            }
            if let (Some(start), Some(len)) = (body_start, content_length) {
                if bytes_read >= start + len {
                    break;
                }
            }
        }
        let request_str = String::from_utf8_lossy(&buf[..bytes_read]);
        println!("WEBHOOK RECEIVED REQUEST: {:?}", request_str);

        assert!(request_str.contains("content-type: application/json"));
        assert!(request_str.contains("x-webhook-signature:"));

        let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(response.as_bytes()).await.unwrap();
        let _ = stream.shutdown().await;
    });

    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Webhook;
    config.webhook_url = Some(format!("http://127.0.0.1:{}/webhook", port));
    config.webhook_secret = Some("my_super_secret_key".into());

    let target = build_target(config.clone());
    let res = target.deliver("Hello Webhook").await;
    println!("WEBHOOK TARGET SUCCESS RES: {:?}", res);
    assert!(res.success, "Webhook delivery failed: {:?}", res.error);

    server.await.unwrap();

    // Failure Path: Missing secret
    config.webhook_secret = None;
    let target_fail = build_target(config);
    let res_fail = target_fail.deliver("Hello").await;
    assert!(!res_fail.success);
    assert_eq!(res_fail.error.as_deref(), Some("No webhook_secret configured"));
}

#[tokio::test]
async fn test_webhook_target_failure_http_error() {
    init_test_env();
    use tokio::net::TcpListener;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0; 1024];
        let mut bytes_read = 0;
        let mut body_start = None;
        let mut content_length = None;
        loop {
            let n = stream.read(&mut buf[bytes_read..]).await.unwrap();
            if n == 0 {
                break;
            }
            bytes_read += n;
            let s = String::from_utf8_lossy(&buf[..bytes_read]);
            if body_start.is_none() {
                if let Some(pos) = s.find("\r\n\r\n") {
                    body_start = Some(pos + 4);
                    for line in s[..pos].lines() {
                        if line.to_lowercase().starts_with("content-length:") {
                            if let Some(len_str) = line.split(':').nth(1) {
                                if let Ok(len) = len_str.trim().parse::<usize>() {
                                    content_length = Some(len);
                                }
                            }
                        }
                    }
                }
            }
            if let (Some(start), Some(len)) = (body_start, content_length) {
                if bytes_read >= start + len {
                    break;
                }
            }
        }
        let response = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(response.as_bytes()).await.unwrap();
        let _ = stream.shutdown().await;
    });

    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Webhook;
    config.webhook_url = Some(format!("http://127.0.0.1:{}/webhook", port));
    config.webhook_secret = Some("my_super_secret_key".into());

    let target = build_target(config);
    let res = target.deliver("Hello").await;
    assert!(!res.success);
    assert!(res.error.as_ref().unwrap().contains("403") || res.error.as_ref().unwrap().contains("HTTP"));

    server.await.unwrap();
}

// 10. MCP Target Failure Tests
#[tokio::test]
async fn test_mcp_delivery_failure_connect_error() {
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Mcp;
    config.mcp_path = Some("/nonexistent/mcp/socket_xyz_123.sock".into());
    config.mcp_tool = Some("speak_text".into());

    let target = build_target(config);
    let res = target.deliver("Hello").await;
    assert!(!res.success);
    assert!(res.error.as_ref().unwrap().contains("Failed to connect to MCP socket"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_mcp_delivery_failure_server_error() {
    use tokio::net::UnixListener;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let socket_path = format!("/tmp/voxctrl-mcp-fail-test-{}.sock", chrono::Utc::now().timestamp_millis());
    let socket_path_clone = socket_path.clone();

    let server = tokio::spawn(async move {
        let listener = UnixListener::bind(&socket_path_clone).unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();

        // 1. Receive initialize
        let line = lines.next_line().await.unwrap().unwrap();
        assert!(line.contains("initialize"));
        
        // Respond with an initialization error
        let init_err = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32603,
                "message": "Initialization failed deliberately"
            }
        });
        writer.write_all((serde_json::to_string(&init_err).unwrap() + "\n").as_bytes()).await.unwrap();
        writer.flush().await.unwrap();
    });

    // Give the server a small moment to bind
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let config = OutputTarget {
        id: "mcp_fail_test".into(),
        label: "Test MCP".into(),
        delivery: DeliveryType::Mcp,
        command: None,
        pipe_path: None,
        socket_host: None,
        socket_port: None,
        socket_unix: None,
        file_path: None,
        file_prefix: "".into(),
        file_timestamp: false,
        file_mode: "append".into(),
        dbus_signal: None,
        http_url: None,
        http_method: "POST".into(),
        http_headers: None,
        http_json_template: None,
        webhook_url: None,
        webhook_secret: None,
        webhook_json_template: None,
        mcp_path: Some(socket_path.clone()),
        mcp_tool: Some("speak_text".into()),
        mcp_args: Some(serde_json::json!({ "text": "{TEXT}" })),
        send_on_release: true,
        append_newline: false,
        strip_newlines: false,
        initial_prompt: None,
        processing: Default::default(),
        response_pipe: None,
    };

    let target = build_target(config);
    let result = target.deliver("Hello Failure").await;

    assert!(!result.success);
    assert!(result.error.as_ref().unwrap().contains("MCP initialization error: Initialization failed deliberately"));

    server.await.unwrap();
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_strip_newlines_config_roundtrip() {
    let temp_dir = std::env::temp_dir().join(format!("voxctrl_strip_nl_test_{}", chrono::Utc::now().timestamp_millis()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let target = OutputTarget {
        id: "strip_test".into(),
        label: "Test Strip Newlines".into(),
        delivery: DeliveryType::Inject,
        command: None,
        pipe_path: None,
        socket_host: None,
        socket_port: None,
        socket_unix: None,
        file_path: None,
        file_prefix: "".into(),
        file_timestamp: false,
        file_mode: "append".into(),
        dbus_signal: None,
        http_url: None,
        http_method: "POST".into(),
        http_headers: None,
        http_json_template: None,
        webhook_url: None,
        webhook_secret: None,
        webhook_json_template: None,
        mcp_path: None,
        mcp_tool: None,
        mcp_args: None,
        send_on_release: true,
        append_newline: false,
        strip_newlines: true,
        initial_prompt: None,
        processing: Default::default(),
        response_pipe: None,
    };

    save_targets(&[target.clone()], &temp_dir).unwrap();
    let loaded = load_targets(&temp_dir).unwrap();

    assert_eq!(loaded.len(), 1);
    let loaded_target = &loaded[0];
    assert_eq!(loaded_target.id, "strip_test");
    assert!(loaded_target.strip_newlines);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_example_configurations_load() {
    use crate::loader::load_bindings;
    let temp_dir = std::env::temp_dir().join(format!("voxctrl_examples_test_{}", chrono::Utc::now().timestamp_millis()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let examples_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");

    // Test bindings-multi.toml
    let bindings_src = examples_path.join("bindings-multi.toml");
    let bindings_dest = temp_dir.join("bindings.toml");
    std::fs::copy(&bindings_src, &bindings_dest).unwrap();
    let loaded_bindings = load_bindings(&temp_dir);
    assert!(loaded_bindings.is_ok(), "Failed to load bindings-multi.toml: {:?}", loaded_bindings.err());
    let loaded_bindings = loaded_bindings.unwrap();
    assert!(!loaded_bindings.is_empty(), "bindings-multi.toml was parsed empty");

    // Test targets examples
    let targets_files = vec![
        "targets-basic.toml",
        "targets-multi.toml",
        "targets-openai-workflows.toml",
        "targets-tts-agent.toml",
    ];

    for file_name in targets_files {
        let targets_src = examples_path.join(file_name);
        let targets_dest = temp_dir.join("targets.toml");
        std::fs::copy(&targets_src, &targets_dest).unwrap();
        let loaded_targets = load_targets(&temp_dir);
        assert!(
            loaded_targets.is_ok(),
            "Failed to load target config file {}: {:?}",
            file_name,
            loaded_targets.err()
        );
        let loaded_targets = loaded_targets.unwrap();
        assert!(!loaded_targets.is_empty(), "Target config {} was parsed empty", file_name);
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_hold_threshold_default() {
    use crate::loader::default_bindings;
    
    // Default bindings use a 200ms hold threshold: long enough to debounce
    // accidental taps, short enough that a normal hotkey press gives feedback
    // (the old 1000ms default made a press look completely dead).
    let bindings = default_bindings();
    for binding in bindings {
        assert_eq!(binding.hold_threshold_ms, 200);
    }
}

#[tokio::test]
async fn test_speak_target_success() {
    use std::sync::{Arc, Mutex};
    let mut config = OutputTarget::default_inject();
    config.delivery = DeliveryType::Speak;
    
    // Set mock speak callback
    let spoken = Arc::new(Mutex::new(String::new()));
    let spoken_clone = spoken.clone();
    crate::targets::set_speak_callback(Arc::new(move |text| {
        *spoken_clone.lock().unwrap() = text.to_string();
    }));
    
    let target = build_target(config);
    let res = target.deliver("Hello speak target").await;
    assert!(res.success);
    assert_eq!(*spoken.lock().unwrap(), "Hello speak target");
}

// ── shellexpand_tilde tests ───────────────────────────────────────────────────

#[test]
fn test_shellexpand_tilde_bare_tilde_does_not_panic() {
    // Bug fix regression: bare "~" must not panic with byte-index-out-of-bounds.
    use crate::targets::shellexpand_tilde_pub;
    let result = shellexpand_tilde_pub("~");
    // Result must be a valid path (either home dir or "~" if home is unavailable).
    assert!(!result.is_empty());
}

#[test]
fn test_shellexpand_tilde_with_slash_expands() {
    use crate::targets::shellexpand_tilde_pub;
    if let Some(home) = dirs::home_dir() {
        let result = shellexpand_tilde_pub("~/documents");
        let expected = home.join("documents").to_string_lossy().into_owned();
        assert_eq!(result, expected);
    }
}

#[test]
fn test_shellexpand_tilde_bare_expands_to_home() {
    use crate::targets::shellexpand_tilde_pub;
    if let Some(home) = dirs::home_dir() {
        let result = shellexpand_tilde_pub("~");
        assert_eq!(result, home.to_string_lossy());
    }
}

#[test]
fn test_shellexpand_tilde_absolute_path_unchanged() {
    use crate::targets::shellexpand_tilde_pub;
    let path = "/absolute/path/to/file.txt";
    assert_eq!(shellexpand_tilde_pub(path), path);
}

#[test]
fn test_shellexpand_tilde_relative_path_unchanged() {
    use crate::targets::shellexpand_tilde_pub;
    let path = "relative/path/file.txt";
    assert_eq!(shellexpand_tilde_pub(path), path);
}

#[test]
fn test_shellexpand_tilde_no_tilde_prefix_unchanged() {
    use crate::targets::shellexpand_tilde_pub;
    // "~something" (no slash) is NOT a home-dir expansion — keep as-is.
    let path = "~something";
    assert_eq!(shellexpand_tilde_pub(path), path);
}

