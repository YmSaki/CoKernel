use rmcp::model::ToolAnnotations;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use serde_json::Value;
pub mod contracts;

/// Canonical annotation shape for bounded, closed-world read operations.
pub fn safe_read_annotations(title: &str) -> ToolAnnotations {
    ToolAnnotations::from_raw(
        Some(title.into()),
        Some(true),
        Some(false),
        Some(true),
        Some(false),
    )
}

/// Baseline transport configuration for CoKernel's MCP endpoint.
///
/// The service itself is implemented in Phase 6. Phase 0 fixes the official
/// SDK and Streamable HTTP path so later work does not need a transport rewrite.
pub fn streamable_http_config() -> StreamableHttpServerConfig {
    StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
}

pub fn get_variable_schema() -> Value {
    contracts::all()
        .into_iter()
        .find(|t| t.name == "get_variable")
        .map(|t| Value::Object(t.tool().input_schema.as_ref().clone()))
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_read_annotations_serialize_with_expected_hints() {
        let value = serde_json::to_value(safe_read_annotations("Get Variable")).unwrap();
        assert_eq!(value["title"], "Get Variable");
        assert_eq!(value["readOnlyHint"], true);
        assert_eq!(value["destructiveHint"], false);
        assert_eq!(value["idempotentHint"], true);
        assert_eq!(value["openWorldHint"], false);
    }

    #[test]
    fn safe_inspection_schema_is_generated_from_validated_contract() {
        let schema = get_variable_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["name"]["type"], "string");
        assert_eq!(schema["properties"]["project_id"]["type"], "string");
        assert_eq!(schema["properties"]["session_id"]["type"], "string");
    }

    #[test]
    fn streamable_http_server_config_is_available() {
        let _ = streamable_http_config();
    }
}
#[cfg(target_os = "linux")]
pub mod auth;
#[cfg(target_os = "linux")]
pub mod http_server;
#[cfg(target_os = "linux")]
pub mod runtime_client;
#[cfg(target_os = "linux")]
pub mod service;
