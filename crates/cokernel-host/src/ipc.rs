//! Host domain dispatch shared by local transports. Client IDs never reach Runtime.
use crate::{bridge::BridgeError, state::DesiredState, supervisor::Host};
use cokernel_domain::DomainError;
use cokernel_protocol::{RequestEnvelope, ResponseEnvelope};
use serde_json::{Value, json};

pub fn error(id: String, code: &str, summary: &str) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol: 1,
        request_id: id,
        ok: false,
        result: None,
        error: Some(DomainError {
            code: code.into(),
            component: "host".into(),
            summary: summary.into(),
            action: "Refresh Host status and resolve the reported condition before retrying".into(),
            detail: None,
        }),
    }
}
pub fn success(id: String, result: Value) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol: 1,
        request_id: id,
        ok: true,
        result: Some(result),
        error: None,
    }
}
fn host_error(id: String, error_value: BridgeError) -> ResponseEnvelope {
    error(id, error_value.code, &error_value.message)
}
pub fn validate(request: &RequestEnvelope) -> Result<(), Box<ResponseEnvelope>> {
    if request.protocol != 1 {
        return Err(Box::new(error(
            request.request_id.clone(),
            "CK-HOST-PROTOCOL",
            "Unsupported Host IPC protocol",
        )));
    }
    if request.request_id.is_empty()
        || request.request_id.len() > 128
        || request.request_id.chars().any(char::is_control)
        || request.method.is_empty()
        || request.method.len() > 128
    {
        return Err(Box::new(error(
            String::new(),
            "CK-HOST-REQUEST",
            "Invalid request ID or method",
        )));
    }
    Ok(())
}
pub async fn dispatch(host: &Host, request: RequestEnvelope) -> ResponseEnvelope {
    if let Err(error) = validate(&request) {
        return *error;
    }
    let id = request.request_id;
    match request.method.as_str() {
        #[cfg(windows)]
        "remote_access.configure"
        | "remote_access.get_configuration"
        | "remote_access.clear_configuration"
        | "remote_access.get_status"
        | "remote_access.start"
        | "remote_access.stop"
        | "remote_access.restart" => {
            crate::remote_access::dispatch(host, id, &request.method, request.params).await
        }
        "resources.get_snapshot" => {
            if request.params != json!({}) {
                return error(
                    id,
                    "CK-HOST-REQUEST",
                    "Resource snapshot takes empty parameters",
                );
            }
            success(id, host.resources().await)
        }
        #[cfg(windows)]
        "host.autostart.get" | "host.autostart.set" => {
            crate::autostart::dispatch(host, id, &request.method, request.params).await
        }
        #[cfg(windows)]
        "notebooks.import_file" => crate::import_file::dispatch(host, id, request.params).await,
        "host.hello" => {
            if request.params != json!({"supported_protocols":[1]}) {
                return error(
                    id,
                    "CK-HOST-PROTOCOL",
                    "Host hello requires supported_protocols [1]",
                );
            }
            let capabilities = vec![
                "host-status",
                "runtime-control",
                "domain-rpc",
                "host-state-events",
                "runtime-events",
                "runtime-snapshot",
                "runtime-diagnostics-forwarding",
                "resource-snapshot",
            ];
            #[cfg(windows)]
            let capabilities = {
                let mut capabilities = capabilities;
                capabilities.push("windows-notebook-import");
                capabilities.push("windows-secret-store");
                capabilities.push("remote-access-control");
                if host.autostart_target().is_some() {
                    capabilities.push("host-autostart");
                }
                capabilities
            };
            success(
                id,
                json!({"selected_protocol":1,"host_product_version":env!("CARGO_PKG_VERSION"),"capabilities":capabilities}),
            )
        }
        "runtime.get_status" | "runtime.get_versions" | "runtime.start" | "runtime.stop" => {
            if request.params != json!({}) {
                return error(
                    id,
                    "CK-HOST-REQUEST",
                    "Runtime control takes empty parameters",
                );
            }
            if request.method == "runtime.get_versions" {
                let status = host.status();
                return success(
                    id,
                    json!({"protocol":1,"host_product_version":env!("CARGO_PKG_VERSION"),"runtime_product_version":status.runtime_version,"runtime_capabilities":status.runtime_capabilities,"generation":status.generation}),
                );
            }
            let result = match request.method.as_str() {
                "runtime.start" => host.set_desired(DesiredState::Running).await,
                "runtime.stop" => host.set_desired(DesiredState::Stopped).await,
                _ => Ok(()),
            };
            match result {
                Ok(()) => success(id, json!(host.status())),
                Err(error) => host_error(id, error),
            }
        }
        method if permitted(method) => match host.call(method, request.params).await {
            Ok(mut response) => {
                response.request_id = id;
                response
            }
            Err(error) => host_error(id, error),
        },
        _ => error(
            id,
            "CK-HOST-NO-METHOD",
            "Unknown or internal-only Host method",
        ),
    }
}
fn permitted(method: &str) -> bool {
    matches!(
        method,
        "projects.list"
            | "runtime.get_snapshot"
            | "runtime.get_diagnostics"
            | "runtime.get_mcp_status"
            | "mcp.restart"
            | "projects.create"
            | "projects.register"
            | "projects.get"
            | "projects.remove_from_registry"
            | "packages.add"
            | "packages.remove"
            | "packages.sync"
            | "environment.get_status"
            | "notebooks.list"
            | "notebooks.create"
            | "notebooks.open"
            | "notebooks.get"
            | "notebooks.read_chunk"
            | "notebooks.apply_edits"
            | "notebooks.save"
            | "notebooks.close"
            | "notebooks.reload"
            | "notebooks.begin_import"
            | "notebooks.import_chunk"
            | "notebooks.finish_import"
            | "notebooks.cancel_import"
            | "sessions.ensure_primary"
            | "sessions.execute_cell"
            | "sessions.restart"
            | "sessions.stop"
            | "sessions.interrupt"
            | "sessions.list"
            | "sessions.get"
            | "sessions.list_variables"
            | "sessions.get_variable"
            | "operations.get"
            | "operations.cancel"
    )
}
