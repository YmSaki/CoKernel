use rmcp::model::ToolAnnotations;
use rmcp::schemars::{JsonSchema, schema_for};
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetVariableParams {
    /// Stable CoKernel Project identifier.
    pub project_id: String,
    /// Stable CoKernel Notebook identifier.
    pub notebook_id: String,
    /// One Python identifier. Expressions are not accepted by the v1 safe-inspection API.
    pub name: String,
}

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
    serde_json::to_value(schema_for!(GetVariableParams))
        .expect("schemars-generated MCP parameter schema must serialize")
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
    fn safe_inspection_schema_is_generated_from_rust_type() {
        let schema = get_variable_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["name"]["type"], "string");
        assert_eq!(schema["properties"]["project_id"]["type"], "string");
        assert_eq!(schema["properties"]["notebook_id"]["type"], "string");
    }

    #[test]
    fn streamable_http_server_config_is_available() {
        let _ = streamable_http_config();
    }
}
