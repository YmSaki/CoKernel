use crate::runtime_client::RuntimeClient;
use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
use serde_json::{Value, json};
#[derive(Clone)]
pub struct CoKernelService {
    pub runtime: RuntimeClient,
}
impl CoKernelService {
    fn tools() -> Vec<Tool> {
        crate::contracts::all().iter().map(|t| t.tool()).collect()
    }
}
impl ServerHandler for CoKernelService {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        info.server_info = Implementation::new("cokernel", env!("CARGO_PKG_VERSION"));
        info
    }
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: Self::tools(),
            ..Default::default()
        })
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        Self::tools().into_iter().find(|t| t.name == name)
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let contract = crate::contracts::all()
            .into_iter()
            .find(|t| t.name == request.name)
            .ok_or_else(|| ErrorData::invalid_params("Unknown CoKernel tool", None))?;
        let args = match contract.validate(request.arguments.unwrap_or_default()) {
            Ok(args) => args,
            Err(message) => {
                return Ok(bounded_result(
                    json!({"code":"CK-MCP-PARAMS","summary":message}),
                    true,
                ));
            }
        };
        let params = if contract.name == "get_runtime_status" {
            json!({"host_product_version":env!("CARGO_PKG_VERSION"),"supported_protocols":[1]})
        } else {
            runtime_params(contract.name, args)
        };
        let method = contract.method;
        let mut response = match self.runtime.call(method, params).await {
            Ok(response) => response,
            Err(_) => {
                return Ok(bounded_result(
                    json!({"code":"CK-MCP-OUTCOME-UNKNOWN","summary":"Runtime reply unavailable; refresh Session/operation status before retrying accepted work"}),
                    true,
                ));
            }
        };
        if response.ok && contract.name == "get_runtime_status" {
            match self.runtime.call("runtime.get_mcp_status", json!({})).await {
                Ok(status) if status.ok => {
                    if let Some(Value::Object(result)) = response.result.as_mut() {
                        result.insert("mcp".into(), status.result.unwrap_or(Value::Null));
                    }
                }
                _ => {
                    return Ok(bounded_result(
                        json!({"code":"CK-MCP-STATUS","summary":"MCP supervisor status unavailable"}),
                        true,
                    ));
                }
            }
        }
        Ok(if response.ok {
            bounded_result(response.result.unwrap_or(Value::Null), false)
        } else {
            bounded_result(
                serde_json::to_value(response.error).unwrap_or(Value::Null),
                true,
            )
        })
    }
}
fn bounded_result(value: Value, error: bool) -> CallToolResponse {
    let text = serde_json::to_string(&value).unwrap_or_default();
    if text.len() > 128 * 1024 {
        return CallToolResult::error(vec![ContentBlock::text("CK-MCP-LIMIT: Response exceeds limit; use a smaller notebook page or inspect individual variables")]).into();
    }
    let result = if error {
        CallToolResult::error(vec![ContentBlock::text(text)])
    } else {
        CallToolResult::success(vec![ContentBlock::text(text)])
    };
    // Include JSON escaping, MCP envelope and annotations in the response budget.
    if serde_json::to_vec(&result).map_or(true, |bytes| bytes.len() > 256 * 1024 - 4096) {
        CallToolResult::error(vec![ContentBlock::text(
            "CK-MCP-LIMIT: Encoded response exceeds limit",
        )])
        .into()
    } else {
        result.into()
    }
}

/// Translate narrow authoring tools into the existing revisioned document API.
fn runtime_params(name: &str, mut args: serde_json::Map<String, Value>) -> Value {
    let edit = match name {
        "insert_code_cell" => Some(
            json!({"op":"insert_cell","index":args.remove("index"),"cell":{"cell_type":"code","id":args.remove("cell_id"),"source":args.remove("source"),"metadata":{},"outputs":[],"execution_count":null}}),
        ),
        "set_cell_source" => Some(
            json!({"op":"set_cell_source","cell_id":args.remove("cell_id"),"source":args.remove("source")}),
        ),
        _ => None,
    };
    if let Some(edit) = edit {
        args.insert("edits".into(), json!([edit]));
    }
    Value::Object(args)
}
#[cfg(test)]
mod authoring_tests {
    use super::*;
    #[test]
    fn edits_preserve_revision_and_never_supply_execution_source() {
        let args = json!({"project_id":"p","notebook_id":"n","expected_revision":3,"index":0,"cell_id":"cell-a","source":"x = 123"});
        let p = runtime_params("insert_code_cell", args.as_object().unwrap().clone());
        assert_eq!(p["expected_revision"], 3);
        assert_eq!(p["edits"][0]["cell"]["source"], "x = 123");
        assert!(p.get("source").is_none());
        assert!(p.get("index").is_none());
        assert_eq!(p["edits"][0]["cell"]["execution_count"], Value::Null);
    }
}
