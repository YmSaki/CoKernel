//! One declarative transport contract drives both advertised schemas and validation.
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Map, Value, json};

#[derive(Clone, Copy)]
enum Kind {
    Id,
    Cell,
    Identifier,
    Number(u64),
    Bool,
}
#[derive(Clone)]
struct Field {
    name: &'static str,
    kind: Kind,
    default: Option<Value>,
    optional: bool,
}
impl Field {
    fn required(name: &'static str, kind: Kind) -> Self {
        Self {
            name,
            kind,
            default: None,
            optional: false,
        }
    }
    fn default(name: &'static str, kind: Kind, value: Value) -> Self {
        Self {
            name,
            kind,
            default: Some(value),
            optional: true,
        }
    }
    fn optional(name: &'static str, kind: Kind) -> Self {
        Self {
            name,
            kind,
            default: None,
            optional: true,
        }
    }
}
pub struct Contract {
    pub name: &'static str,
    pub method: &'static str,
    description: &'static str,
    fields: Vec<Field>,
    hints: (bool, bool, bool, bool),
}
fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c=='_' || unicode_ident::is_xid_start(c))
        && chars.all(unicode_ident::is_xid_continue)
        && !matches!(
            name,
            "False"
                | "None"
                | "True"
                | "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "try"
                | "while"
                | "with"
                | "yield"
        )
}
impl Contract {
    pub fn tool(&self) -> Tool {
        let properties: Map<String,Value> = self.fields.iter().map(|f| {
            let mut s=match f.kind {
                Kind::Id=>json!({"type":"string","format":"uuid","minLength":36,"maxLength":36}),
                Kind::Cell=>json!({"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Za-z0-9_-]+$"}),
                Kind::Identifier=>json!({"type":"string","minLength":1,"maxLength":256,"description":"One Python identifier, not a keyword or expression. Worker validates again."}),
                Kind::Number(max)=>json!({"type":"integer","minimum":if f.name=="limit" {1} else {0},"maximum":max}),
                Kind::Bool=>json!({"type":"boolean"}),
            };
            if let Some(default)=&f.default {s["default"]=default.clone();}
            (f.name.into(),s)
        }).collect();
        let required: Vec<_> = self
            .fields
            .iter()
            .filter(|f| !f.optional)
            .map(|f| f.name)
            .collect();
        let schema = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
        let mut tool = Tool::new(
            self.name,
            self.description,
            schema.as_object().cloned().unwrap_or_default(),
        );
        let (read, destructive, idempotent, open) = self.hints;
        tool.annotations = Some(ToolAnnotations::from_raw(
            Some(self.name.into()),
            Some(read),
            Some(destructive),
            Some(idempotent),
            Some(open),
        ));
        tool
    }
    pub fn validate(
        &self,
        mut args: Map<String, Value>,
    ) -> Result<Map<String, Value>, &'static str> {
        if args
            .keys()
            .any(|k| !self.fields.iter().any(|f| f.name == k))
        {
            return Err("Unknown argument");
        }
        for field in &self.fields {
            if !args.contains_key(field.name) {
                if let Some(default) = &field.default {
                    args.insert(field.name.into(), default.clone());
                } else if field.optional {
                    continue;
                } else {
                    return Err("Missing required argument");
                }
            }
            let value = &args[field.name];
            let valid = match field.kind {
                Kind::Id => value
                    .as_str()
                    .is_some_and(|s| s.len() == 36 && uuid::Uuid::parse_str(s).is_ok()),
                Kind::Cell => value.as_str().is_some_and(|s| {
                    !s.is_empty()
                        && s.len() <= 64
                        && s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                }),
                Kind::Identifier => value
                    .as_str()
                    .is_some_and(|s| s.chars().count() <= 256 && identifier(s)),
                Kind::Number(max) => value
                    .as_u64()
                    .is_some_and(|n| n <= max && (field.name != "limit" || n > 0)),
                Kind::Bool => value.is_boolean(),
            };
            if !valid {
                return Err("Invalid argument type, identifier or bound");
            }
        }
        Ok(args)
    }
}
pub fn all() -> Vec<Contract> {
    use Kind::*;
    let project = || vec![Field::required("project_id", Id)];
    let notebook = || {
        vec![
            Field::required("project_id", Id),
            Field::required("notebook_id", Id),
        ]
    };
    let session = || {
        vec![
            Field::required("project_id", Id),
            Field::required("session_id", Id),
        ]
    };
    let read = (true, false, true, false);
    let mut tools = Vec::new();
    let mut add = |name, method, description, fields, hints| {
        tools.push(Contract {
            name,
            method,
            description,
            fields,
            hints,
        })
    };
    add(
        "list_projects",
        "projects.list",
        "List uv Projects registered with this Runtime.",
        vec![],
        read,
    );
    let mut fields = project();
    fields.extend([
        Field::default("offset", Number(100_000), json!(0)),
        Field::default("limit", Number(128), json!(32)),
    ]);
    add(
        "list_notebooks",
        "notebooks.list",
        "List notebook paths and identities of currently open documents. Closed paths have no live document ID; open them in Desktop first.",
        fields,
        read,
    );
    let mut fields = notebook();
    fields.extend([
        Field::optional("expected_revision", Number(9_007_199_254_740_991)),
        Field::default("offset", Number(10_000), json!(0)),
        Field::default("limit", Number(32), json!(16)),
        Field::default("include_outputs", Bool, json!(false)),
    ]);
    add(
        "read_notebook",
        "notebooks.read_cells",
        "Read a bounded cell page from an open notebook. Use the returned revision for subsequent pages/execution. Truncation is explicit; outputs are opt-in.",
        fields,
        read,
    );
    add(
        "list_sessions",
        "sessions.list",
        "List Sessions in one Project, including primary notebook identities and state.",
        project(),
        read,
    );
    add(
        "get_session",
        "sessions.get",
        "Read Session state without starting it.",
        session(),
        read,
    );
    add(
        "ensure_session",
        "sessions.ensure_primary",
        "Return the notebook's shared primary Session, starting it only if needed. Human and AI use the same namespace.",
        notebook(),
        (false, false, true, false),
    );
    let mut fields = notebook();
    fields.extend([
        Field::required("session_id", Id),
        Field::required("cell_id", Cell),
        Field::required("expected_revision", Number(9_007_199_254_740_991)),
    ]);
    add(
        "execute_cell",
        "sessions.execute_cell",
        "Execute authoritative stored cell source in the shared FIFO. Code can have arbitrary effects. Returns an operation ID; use get_operation for completion. Never blindly retry an uncertain result.",
        fields,
        (false, true, false, true),
    );
    add(
        "interrupt_session",
        "sessions.interrupt",
        "Interrupt the current operation, including one started by the human. Previously executed effects remain.",
        session(),
        (false, true, true, false),
    );
    add(
        "restart_session",
        "sessions.restart",
        "Explicitly restart the notebook primary Session, losing its volatile variables; saved notebook files are retained.",
        notebook(),
        (false, true, false, false),
    );
    add(
        "stop_session",
        "sessions.stop",
        "Stop this Session and lose volatile memory. Notebook files are retained.",
        session(),
        (false, true, true, false),
    );
    add(
        "list_variables",
        "sessions.list_variables",
        "Read bounded variable metadata without evaluating repr or user expressions.",
        session(),
        read,
    );
    let mut fields = session();
    fields.push(Field::required("name", Identifier));
    add(
        "get_variable",
        "sessions.get_variable",
        "Read one identifier using exact-type bounded serialization in the existing Session. Custom objects are not evaluated.",
        fields,
        read,
    );
    add(
        "get_runtime_status",
        "bridge.hello",
        "Read Runtime protocol/version/capabilities without starting a Session.",
        vec![],
        read,
    );
    add(
        "get_resources",
        "runtime.get_linux_metrics",
        "Read cached Linux/WSL resources. Unavailable/stale values are explicit; these are not Windows host totals.",
        vec![],
        read,
    );
    let mut fields = project();
    fields.push(Field::required("operation_id", Id));
    add(
        "get_operation",
        "operations.get",
        "Read execution completion and persistence status by operation ID, scoped to its Project.",
        fields,
        read,
    );
    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_contract_matches_reviewed_snapshot() {
        let tools: Vec<_> = all().iter().map(|c| c.tool()).collect();
        let snapshot: Value =
            serde_json::from_str(include_str!("../../../protocol/mcp/tools.json")).unwrap();
        assert_eq!(serde_json::to_value(tools).unwrap(), snapshot);
    }
    #[test]
    fn minimum_tools_and_exact_annotations() {
        let tools = all();
        assert_eq!(tools.len(), 15);
        for tool in tools {
            let hints = tool.tool().annotations.unwrap();
            assert_eq!(
                hints.read_only_hint,
                Some(!matches!(
                    tool.name,
                    "ensure_session"
                        | "execute_cell"
                        | "interrupt_session"
                        | "restart_session"
                        | "stop_session"
                ))
            );
            assert_eq!(
                hints.destructive_hint,
                Some(matches!(
                    tool.name,
                    "execute_cell" | "interrupt_session" | "restart_session" | "stop_session"
                ))
            );
            assert_eq!(
                hints.idempotent_hint,
                Some(!matches!(tool.name, "execute_cell" | "restart_session"))
            );
            assert_eq!(hints.open_world_hint, Some(tool.name == "execute_cell"));
        }
    }
    #[test]
    fn expressions_keywords_unknown_fields_and_invalid_ids_fail_before_runtime() {
        let tool = all()
            .into_iter()
            .find(|t| t.name == "get_variable")
            .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        for name in [
            "a.b",
            "a[0]",
            "f()",
            "a + 1",
            "[x for x in y]",
            "__import__('os')",
            "class",
            "",
        ] {
            assert!(
                tool.validate(
                    serde_json::from_value(json!({"project_id":id,"session_id":id,"name":name}))
                        .unwrap()
                )
                .is_err()
            );
        }
        for name in ["x", "_x", "変数"] {
            assert!(
                tool.validate(
                    serde_json::from_value(json!({"project_id":id,"session_id":id,"name":name}))
                        .unwrap()
                )
                .is_ok()
            );
        }
        assert!(
            tool.validate(
                serde_json::from_value(json!({"project_id":"bad","session_id":id,"name":"x"}))
                    .unwrap()
            )
            .is_err()
        );
        let execute = all()
            .into_iter()
            .find(|t| t.name == "execute_cell")
            .unwrap();
        for extra in ["source", "code", "origin", "path"] {
            let mut args = json!({"project_id":id,"session_id":id,"notebook_id":id,"cell_id":"a","expected_revision":1});
            args[extra] = json!("injected");
            assert!(execute.validate(args.as_object().unwrap().clone()).is_err());
        }
    }
}
