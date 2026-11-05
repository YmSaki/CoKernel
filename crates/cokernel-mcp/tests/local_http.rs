#![cfg(target_os = "linux")]
use cokernel_mcp::{auth::Bearer, http_server::McpServer, runtime_client::RuntimeClient};
use cokernel_runtime::{
    endpoint::RuntimeOwner,
    project::ProjectManager,
    service::RuntimeService,
    unix_api::UnixApi,
    uv::{UvCommandResult, UvError, UvInvocation, UvRunner},
};
use rmcp::{
    ServiceExt,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::json;
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    time::Duration,
};
struct FakeUv;
impl UvRunner for FakeUv {
    fn run(&self, _: &UvInvocation) -> Result<UvCommandResult, UvError> {
        panic!("read-only MCP proof must not run uv")
    }
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("ckmcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn bearer_persists_and_rejects_unsafe_files_without_echoing_it() {
    let dir = Directory::new();
    let path = dir.0.join("bearer");
    let first = Bearer::load_or_create(&path).unwrap();
    let bytes = std::fs::read_to_string(&path).unwrap();
    assert_eq!(bytes.len(), 64);
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    let header = format!("Bearer {bytes}");
    assert!(first.accepts(Some(&header)));
    assert!(
        Bearer::load_or_create(&path)
            .unwrap()
            .accepts(Some(&header))
    );
    assert!(!first.accepts(None));
    assert!(!first.accepts(Some("Bearer wrong")));
    let mut altered = bytes.into_bytes();
    altered[63] = if altered[63] == b'0' { b'1' } else { b'0' };
    assert!(!first.accepts(Some(&format!(
        "Bearer {}",
        String::from_utf8(altered).unwrap()
    ))));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Bearer::load_or_create(&path).is_err());
    let alias = dir.0.join("alias");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    assert!(Bearer::load_or_create(&alias).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&path, b"invalid").unwrap();
    assert!(Bearer::load_or_create(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid");
}

#[tokio::test]
async fn sdk_interoperates_and_auth_origin_limits_restart_preserve_runtime() {
    let dir = Directory::new();
    let socket = dir.0.join("runtime.sock");
    let bearer = dir.0.join("bearer");
    let manager =
        ProjectManager::load(dir.0.join("projects"), dir.0.join("registry.json"), FakeUv).unwrap();
    let (runtime, owner) = RuntimeOwner::start(RuntimeService::new(manager));
    let api = UnixApi::bind(&socket, runtime.clone()).await.unwrap();
    assert!(
        UnixApi::bind(&socket, runtime.clone()).await.is_err(),
        "live API was replaced"
    );
    let client = RuntimeClient::new(socket.clone()).unwrap();
    assert_eq!(client.hello().await.unwrap().selected_protocol, 1);
    assert!(
        McpServer::bind(
            ([0, 0, 0, 0], 0).into(),
            Bearer::load_or_create(&bearer).unwrap(),
            client.clone()
        )
        .await
        .is_err()
    );
    let server = McpServer::bind(
        ([127, 0, 0, 1], 0).into(),
        Bearer::load_or_create(&bearer).unwrap(),
        client.clone(),
    )
    .await
    .unwrap();
    let token = std::fs::read_to_string(&bearer).unwrap();
    let url = format!("http://{}/mcp", server.address);
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    for method in [
        reqwest::Method::GET,
        reqwest::Method::POST,
        reqwest::Method::DELETE,
    ] {
        assert_eq!(
            http.request(method, &url).send().await.unwrap().status(),
            401
        );
    }
    assert_eq!(
        http.post(&url)
            .bearer_auth("wrong")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        http.post(&url)
            .bearer_auth(&token)
            .header("Origin", "https://untrusted.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let body = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"legacy-proof","version":"1"}}});
    let legacy = http
        .post(&url)
        .bearer_auth(&token)
        .header("Accept", "application/json, text/event-stream")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(legacy.status().is_success(), "legacy initialization failed");
    let legacy: serde_json::Value = legacy.json().await.unwrap();
    assert_eq!(legacy["result"]["protocolVersion"], "2025-11-25");
    for (header, value, status) in [
        ("Host", "untrusted.example", 403),
        ("Origin", "null", 403),
        ("Authorization", "Bearer duplicate", 401),
    ] {
        assert_eq!(
            http.post(&url)
                .bearer_auth(&token)
                .header(header, value)
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            status
        );
    }
    let oversized = http
        .post(&url)
        .bearer_auth(&token)
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .body("x".repeat(256 * 1024 + 1))
        .send()
        .await
        .unwrap();
    assert_eq!(oversized.status(), 413);
    for id in [
        json!("x".repeat(129)),
        json!("bad\nidentity"),
        json!(null),
        json!(9_007_199_254_740_992_u64),
    ] {
        let mut request = body.clone();
        request["id"] = id;
        assert_eq!(
            http.post(&url)
                .bearer_auth(&token)
                .header("Accept", "application/json, text/event-stream")
                .json(&request)
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
    }
    let transport = StreamableHttpClientTransport::with_client(
        http.clone(),
        StreamableHttpClientTransportConfig::with_uri(url).auth_header(token.clone()),
    );
    let sdk = ().serve(transport).await.unwrap();
    let tools = sdk.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 23);
    for tool in tools {
        assert_eq!(
            tool.annotations.unwrap().open_world_hint,
            Some(matches!(
                tool.name.as_ref(),
                "execute_cell" | "create_project" | "add_packages" | "sync_environment"
            ))
        );
        assert_eq!(
            tool.input_schema.get("additionalProperties"),
            Some(&json!(false))
        );
    }
    let result = sdk
        .call_tool(rmcp::model::CallToolRequestParams::new("list_projects"))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    let status = sdk
        .call_tool(rmcp::model::CallToolRequestParams::new(
            "get_runtime_status",
        ))
        .await
        .unwrap();
    assert_eq!(status.is_error, Some(false));
    let bad_args = sdk
        .call_tool(
            rmcp::model::CallToolRequestParams::new("list_projects")
                .with_arguments(serde_json::from_value(json!({"path":"/etc/shadow"})).unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(bad_args.is_error, Some(true));
    sdk.cancel().await.unwrap();
    server.shutdown().await.unwrap();
    assert!(
        client.call("bridge.ping", json!({})).await.unwrap().ok,
        "MCP shutdown stopped Runtime"
    );
    let restarted = McpServer::bind(
        ([127, 0, 0, 1], 0).into(),
        Bearer::load_or_create(&bearer).unwrap(),
        client.clone(),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read_to_string(&bearer).unwrap(), token);
    restarted.shutdown().await.unwrap();
    assert!(
        !client.call("bridge.shutdown", json!({})).await.unwrap().ok,
        "MCP Unix API could stop whole Runtime"
    );
    assert!(
        !client.call("mcp.restart", json!({})).await.unwrap().ok,
        "MCP Unix API could restart its own supervisor"
    );
    api.shutdown().await.unwrap();
    assert!(!socket.exists());
    owner.finish().await.unwrap();
}

#[tokio::test]
async fn unix_api_recovers_only_stale_owned_sockets_and_rejects_unsafe_paths() {
    let dir = Directory::new();
    let path = dir.0.join("runtime.sock");
    let manager =
        ProjectManager::load(dir.0.join("projects"), dir.0.join("registry.json"), FakeUv).unwrap();
    let (runtime, owner) = RuntimeOwner::start(RuntimeService::new(manager));
    std::fs::write(&path, b"preserve").unwrap();
    assert!(UnixApi::bind(&path, runtime.clone()).await.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"preserve");
    std::fs::remove_file(&path).unwrap();
    let stale = std::os::unix::net::UnixListener::bind(&path).unwrap();
    drop(stale);
    let api = UnixApi::bind(&path, runtime.clone()).await.unwrap();
    let client = RuntimeClient::new(path.clone()).unwrap();
    assert!(client.call("bridge.ping", json!({})).await.unwrap().ok);
    api.shutdown().await.unwrap();
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(UnixApi::bind(&path, runtime.clone()).await.is_err());
    owner.finish().await.unwrap();
}

#[tokio::test]
async fn malformed_and_fragmented_unix_clients_do_not_block_other_requests() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = Directory::new();
    let path = dir.0.join("runtime.sock");
    let manager =
        ProjectManager::load(dir.0.join("projects"), dir.0.join("registry.json"), FakeUv).unwrap();
    let (runtime, owner) = RuntimeOwner::start(RuntimeService::new(manager));
    let api = UnixApi::bind(&path, runtime).await.unwrap();
    let client = RuntimeClient::new(path.clone()).unwrap();
    for size in [0_u32, 1024 * 1024 + 1] {
        let mut stream = tokio::net::UnixStream::connect(&path).await.unwrap();
        stream.write_u32(size).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), stream.read_u8())
                .await
                .unwrap()
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }
    let mut slow = tokio::net::UnixStream::connect(&path).await.unwrap();
    slow.write_all(&[0, 0]).await.unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_secs(1),
            client.call("bridge.ping", json!({}))
        )
        .await
        .unwrap()
        .unwrap()
        .ok
    );
    let request = cokernel_protocol::RequestEnvelope {
        protocol: 1,
        request_id: "fragmented".into(),
        method: "bridge.ping".into(),
        params: json!({}),
    };
    let frame = cokernel_protocol::encode_json_frame(&request, 1024).unwrap();
    slow.write_all(&frame[2..]).await.unwrap();
    let size = slow.read_u32().await.unwrap() as usize;
    assert!(size < 1024);
    let mut bytes = vec![0; size];
    slow.read_exact(&mut bytes).await.unwrap();
    let reply: cokernel_protocol::ResponseEnvelope = serde_json::from_slice(&bytes).unwrap();
    assert!(reply.ok);
    assert_eq!(reply.request_id, "fragmented");
    for method in [
        "bridge.shutdown",
        "projects.remove_from_registry",
        "remote_access.configure",
    ] {
        assert_eq!(
            client
                .call(method, json!({}))
                .await
                .unwrap()
                .error
                .unwrap()
                .code,
            "CK-RUN-NO-METHOD"
        );
    }
    // Authoring passes the transport allowlist. This minimal fixture has no
    // NotebookRuntime, so notebook admission reports unavailable (not forbidden).
    for (method, expected) in [
        ("projects.create", "CK-RUN-BAD-PARAMS"),
        ("notebooks.apply_edits", "CK-RUN-404"),
    ] {
        assert_eq!(
            client
                .call(method, json!({}))
                .await
                .unwrap()
                .error
                .unwrap()
                .code,
            expected
        );
    }
    api.shutdown().await.unwrap();
    owner.finish().await.unwrap();
}

#[tokio::test]
async fn runtime_client_rejects_wrong_correlation_shape_and_oversized_replies() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = Directory::new();
    let path = dir.0.join("runtime.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let client = RuntimeClient::new(path).unwrap();
    for variant in 0..4 {
        let peer = tokio::spawn({
            let client = client.clone();
            async move { client.call("bridge.ping", json!({})).await }
        });
        let (mut socket, _) = listener.accept().await.unwrap();
        let length = socket.read_u32().await.unwrap();
        let mut bytes = vec![0; length as usize];
        socket.read_exact(&mut bytes).await.unwrap();
        let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        if variant == 3 {
            socket.write_u32(8 * 1024 * 1024 + 1).await.unwrap();
        } else {
            let mut response = json!({"protocol":1,"request_id":request["request_id"],"ok":true,"result":{},"error":null});
            match variant {
                0 => response["request_id"] = json!("another-client"),
                1 => response["protocol"] = json!(2),
                _ => response["result"] = serde_json::Value::Null,
            }
            let frame = cokernel_protocol::encode_json_frame(&response, 4096).unwrap();
            socket.write_all(&frame).await.unwrap();
        }
        assert!(peer.await.unwrap().is_err());
    }
}
