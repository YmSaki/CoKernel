use cokernel_domain::ProjectId;
use serde::{Deserialize, Serialize};

pub mod name {
    pub const BRIDGE_HELLO: &str = "bridge.hello";
    pub const BRIDGE_PING: &str = "bridge.ping";
    pub const BRIDGE_SHUTDOWN: &str = "bridge.shutdown";

    pub const PROJECTS_LIST: &str = "projects.list";
    pub const PROJECTS_CREATE: &str = "projects.create";
    pub const PROJECTS_REGISTER: &str = "projects.register";
    pub const PROJECTS_GET: &str = "projects.get";
    pub const PROJECTS_REMOVE_FROM_REGISTRY: &str = "projects.remove_from_registry";

    pub const PACKAGES_ADD: &str = "packages.add";
    pub const PACKAGES_REMOVE: &str = "packages.remove";
    pub const PACKAGES_SYNC: &str = "packages.sync";
    pub const ENVIRONMENT_GET_STATUS: &str = "environment.get_status";

    pub const SESSIONS_LIST: &str = "sessions.list";
    pub const SESSIONS_ENSURE_PRIMARY: &str = "sessions.ensure_primary";
    pub const SESSIONS_EXECUTE_CELL: &str = "sessions.execute_cell";
    pub const SESSIONS_INTERRUPT: &str = "sessions.interrupt";
    pub const SESSIONS_RESTART: &str = "sessions.restart";
    pub const SESSIONS_STOP: &str = "sessions.stop";
    pub const SESSIONS_LIST_VARIABLES: &str = "sessions.list_variables";
    pub const SESSIONS_GET_VARIABLE: &str = "sessions.get_variable";
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeHelloParams {
    pub host_product_version: String,
    pub supported_protocols: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeHelloResult {
    pub selected_protocol: u32,
    pub runtime_product_version: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateProjectParams {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegisterProjectParams {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectIdParams {
    pub project_id: ProjectId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageMutationParams {
    pub project_id: ProjectId,
    pub packages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnsurePrimarySessionParams {
    pub project_id: ProjectId,
    pub notebook_id: cokernel_domain::NotebookId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GetVariableParams {
    pub session_id: cokernel_domain::SessionId,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_names_match_v1_contract() {
        assert_eq!(name::PROJECTS_CREATE, "projects.create");
        assert_eq!(name::PACKAGES_SYNC, "packages.sync");
        assert_eq!(name::SESSIONS_ENSURE_PRIMARY, "sessions.ensure_primary");
    }

    #[test]
    fn project_id_params_are_wire_uuid_strings() {
        let id = ProjectId::new();
        let value = serde_json::to_value(ProjectIdParams { project_id: id }).unwrap();
        assert_eq!(value["project_id"], id.to_string());
    }
}
