//! Document handlers run only inside the document owner's FIFO job.
use super::*;
use crate::notebook::{DocumentSnapshot, Edit, NotebookError, NotebookService};
use base64::{Engine, engine::general_purpose::STANDARD};
use cokernel_domain::{ImportId, NotebookId, Project, ProjectId};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathParams {
    project_id: ProjectId,
    relative_path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotebookParams {
    project_id: ProjectId,
    notebook_id: NotebookId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionParams {
    project_id: ProjectId,
    notebook_id: NotebookId,
    expected_revision: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditsParams {
    project_id: ProjectId,
    notebook_id: NotebookId,
    expected_revision: u64,
    edits: Vec<Edit>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeginImportParams {
    project_id: ProjectId,
    target_path: String,
    size_bytes: u64,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportParams {
    project_id: ProjectId,
    import_id: ImportId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportChunkParams {
    project_id: ProjectId,
    import_id: ImportId,
    offset: u64,
    data_base64: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadChunkParams {
    project_id: ProjectId,
    notebook_id: NotebookId,
    expected_revision: u64,
    offset: usize,
    length: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadCellsParams {
    project_id: ProjectId,
    notebook_id: NotebookId,
    expected_revision: Option<u64>,
    #[serde(default)]
    offset: usize,
    #[serde(default = "cell_page_size")]
    limit: usize,
    #[serde(default)]
    include_outputs: bool,
}
fn cell_page_size() -> usize {
    16
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListParams {
    project_id: ProjectId,
    #[serde(default)]
    offset: usize,
    #[serde(default = "page_size")]
    limit: usize,
}
fn page_size() -> usize {
    256
}

fn summary(snapshot: DocumentSnapshot) -> Result<Value, DomainError> {
    Ok(
        json!({"document":snapshot.document,"state":snapshot.state,"content_hash":snapshot.content_hash,"saved_content_hash":snapshot.saved_content_hash}),
    )
}
fn notebook_error(error: NotebookError) -> DomainError {
    error.domain_error()
}
impl<R: UvRunner + 'static> RuntimeService<R> {
    pub(super) fn admit_document(&mut self, request: RequestEnvelope) -> Result<(), DomainError> {
        self.reserve_response(&request.request_id)?;
        let project_id: ProjectId = parse_params(
            request
                .params
                .get("project_id")
                .cloned()
                .unwrap_or(Value::Null),
        )?;
        self.ensure_no_project_removal(project_id)?;
        let project = self
            .projects
            .get(project_id)
            .map_err(project_error)?
            .project;
        let runtime = self
            .notebooks
            .as_ref()
            .ok_or_else(|| notebook_error(NotebookError::NotFound))?;
        let events = runtime.sessions.events();
        let request_id = request.request_id.clone();
        let ticket = runtime.document_queue.submit(move |documents| {
            let method = request.method;
            let ids = json!({"project_id":project_id,"notebook_id":request.params.get("notebook_id")});
            let result = execute(documents, project, &method, request.params);
            if matches!(method.as_str(), "notebooks.create" | "notebooks.open" | "notebooks.apply_edits" | "notebooks.save" | "notebooks.reload" | "notebooks.close" | "notebooks.finish_import") {
                match &result {
                    Ok(summary) => events.publish("notebook.changed", json!({"method":method,"summary":summary})),
                    Err(_) => events.publish("notebook.refresh_required", ids),
                }
            }
            let result = result.and_then(|value| {
                let response = success_response(request.request_id, value);
                if cokernel_protocol::encode_json_frame(&response, cokernel_protocol::DEFAULT_MAX_FRAME_BYTES).is_err() {
                    return Err(notebook_error(NotebookError::Limit));
                }
                Ok(response.result.unwrap_or(Value::Null))
            });
            Ok(result)
        }).map_err(notebook_error)?;
        self.schedule_reply(
            request_id,
            Box::pin(async move { ticket.completed().await.map_err(notebook_error)? }),
            Some(project_id),
            None,
            false,
        );
        Ok(())
    }

    pub(super) async fn admit_snapshot(
        &mut self,
        request: RequestEnvelope,
    ) -> Result<(), DomainError> {
        self.reserve_response(&request.request_id)?;
        if request.params != json!({}) {
            return Err(notebook_error(NotebookError::Invalid(
                "Snapshot takes empty parameters".into(),
            )));
        }
        let runtime = self
            .notebooks
            .as_ref()
            .ok_or_else(|| notebook_error(NotebookError::NotFound))?;
        let events = runtime.sessions.events();
        let before = events.watermark();
        let projects = self.projects.list();
        let sessions = runtime.sessions.list().await;
        let operations = self.packages.snapshot();
        let requests = self.requests.snapshot();
        let mcp = self.mcp.snapshot();
        let tunnel = self.tunnel.snapshot();
        let ticket = runtime.document_queue.submit(move |documents| {
            Ok(json!({"events_before":before,"events_after":events.watermark(),"project_total":projects.len(),"session_total":sessions.len(),"projects":projects.into_iter().take(256).collect::<Vec<_>>(),"sessions":sessions.into_iter().take(512).collect::<Vec<_>>(),"documents":documents.open_summaries(),"package_operations":operations,"request_work":requests,"session_requests":requests,"mcp":mcp,"tunnel":tunnel}))
        }).map_err(notebook_error)?;
        self.schedule_reply(
            request.request_id,
            Box::pin(async move { ticket.completed().await.map_err(notebook_error) }),
            None,
            None,
            true,
        );
        Ok(())
    }

    pub(super) async fn admit_execution(
        &mut self,
        request: RequestEnvelope,
        origin: cokernel_domain::ExecutionOrigin,
    ) -> Result<(), DomainError> {
        self.reserve_response(&request.request_id)?;
        let p: cokernel_protocol::methods::ExecuteNotebookCellParams =
            parse_params(request.params)?;
        self.ensure_no_project_removal(p.project_id)?;
        self.ensure_lifecycle_idle(p.notebook_id)?;
        let project = self
            .projects
            .get(p.project_id)
            .map_err(project_error)?
            .project;
        let runtime = self
            .notebooks
            .as_ref()
            .ok_or_else(|| notebook_error(NotebookError::NotFound))?;
        let prepared = runtime
            .prepare_execution(
                &project,
                p.session_id,
                p.notebook_id,
                &p.cell_id,
                p.expected_revision,
                origin,
            )
            .await?;
        self.schedule_reply(
            request.request_id,
            Box::pin(async move { to_value(prepared.completed().await?) }),
            Some(p.project_id),
            Some(p.notebook_id),
            true,
        );
        Ok(())
    }
}
pub(super) fn execute(
    documents: &mut NotebookService,
    project: Project,
    method: &str,
    params: Value,
) -> Result<Value, DomainError> {
    match method {
        "notebooks.list" => {
            let p: ListParams = parse_params(params)?;
            if p.limit == 0 || p.limit > 512 {
                return Err(notebook_error(NotebookError::Limit));
            }
            let _validated_project = p.project_id;
            let paths = documents.list(&project).map_err(notebook_error)?;
            let total = paths.len();
            let paths: Vec<_> = paths.into_iter().skip(p.offset).take(p.limit).collect();
            let open: Vec<_> = documents
                .open_summaries()
                .into_iter()
                .filter(|s| {
                    s["document"]["project_id"] == json!(project.project_id)
                        && s["document"]["relative_path"]
                            .as_str()
                            .is_some_and(|path| paths.iter().any(|p| p == path))
                })
                .collect();
            Ok(json!({"total":total,"offset":p.offset,"paths":paths,"open_documents":open}))
        }
        "notebooks.read_cells" => {
            let p: ReadCellsParams = parse_params(params)?;
            let _validated_project = p.project_id;
            let snapshot = documents
                .get(&project, p.notebook_id)
                .map_err(notebook_error)?;
            crate::notebook::view::cells(
                snapshot,
                p.expected_revision,
                p.offset,
                p.limit,
                p.include_outputs,
            )
            .map_err(notebook_error)
        }
        "notebooks.create" | "notebooks.open" => {
            let p: PathParams = parse_params(params)?;
            let _validated_project = p.project_id;
            let docs = &mut *documents;
            summary(
                if method == "notebooks.create" {
                    docs.create(&project, &p.relative_path)
                } else {
                    docs.open(&project, &p.relative_path)
                }
                .map_err(notebook_error)?,
            )
        }
        "notebooks.get" => {
            let p: NotebookParams = parse_params(params)?;
            let _validated_project = p.project_id;
            to_value(
                documents
                    .get(&project, p.notebook_id)
                    .map_err(notebook_error)?,
            )
        }
        "notebooks.apply_edits" => {
            let p: EditsParams = parse_params(params)?;
            let _validated_project = p.project_id;
            summary(
                documents
                    .apply_edits(&project, p.notebook_id, p.expected_revision, &p.edits)
                    .map_err(notebook_error)?,
            )
        }
        "notebooks.save" | "notebooks.reload" | "notebooks.close" => {
            let p: RevisionParams = parse_params(params)?;
            let _validated_project = p.project_id;
            let docs = &mut *documents;
            summary(
                match method {
                    "notebooks.reload" => docs.reload(&project, p.notebook_id, p.expected_revision),
                    "notebooks.close" => docs.close(&project, p.notebook_id, p.expected_revision),
                    _ => docs.save(&project, p.notebook_id, p.expected_revision),
                }
                .map_err(notebook_error)?,
            )
        }
        "notebooks.begin_import" => {
            let p: BeginImportParams = parse_params(params)?;
            let _validated_project = p.project_id;
            to_value(
                documents
                    .begin_import(&project, &p.target_path, p.size_bytes, &p.sha256)
                    .map_err(notebook_error)?,
            )
        }
        "notebooks.import_chunk" => {
            let p: ImportChunkParams = parse_params(params)?;
            let _validated_project = p.project_id;
            let decoded = if p.data_base64.len() > 349_528 {
                Err(NotebookError::Limit)
            } else {
                STANDARD.decode(&p.data_base64).map_err(|_| {
                    NotebookError::Import("invalid base64; transaction cancelled".into())
                })
            };
            let docs = &mut *documents;
            let bytes = match decoded {
                Ok(bytes) => bytes,
                Err(error) => {
                    // Ownership is checked before aborting: another Project
                    // must not be able to cancel this transaction.
                    docs.cancel_import(&project, p.import_id)
                        .map_err(notebook_error)?;
                    return Err(notebook_error(error));
                }
            };
            let offset = docs
                .import_chunk(&project, p.import_id, p.offset, &bytes)
                .map_err(notebook_error)?;
            Ok(json!({"next_offset":offset}))
        }
        "notebooks.finish_import" | "notebooks.cancel_import" => {
            let p: ImportParams = parse_params(params)?;
            let _validated_project = p.project_id;
            let docs = &mut *documents;
            if method == "notebooks.finish_import" {
                summary(
                    docs.finish_import(&project, p.import_id)
                        .map_err(notebook_error)?,
                )
            } else {
                docs.cancel_import(&project, p.import_id)
                    .map_err(notebook_error)?;
                Ok(json!({"cancelled":true}))
            }
        }
        "notebooks.read_chunk" => {
            let p: ReadChunkParams = parse_params(params)?;
            if p.length == 0 || p.length > 262_144 {
                return Err(notebook_error(NotebookError::Limit));
            }
            let _validated_project = p.project_id;
            let snapshot = documents
                .get(&project, p.notebook_id)
                .map_err(notebook_error)?;
            if snapshot.document.revision != p.expected_revision {
                return Err(notebook_error(NotebookError::Conflict {
                    current: snapshot.document.revision,
                }));
            }
            let bytes = snapshot.notebook.to_bytes().map_err(notebook_error)?;
            if p.offset > bytes.len() {
                return Err(notebook_error(NotebookError::Limit));
            }
            let end = p.offset.saturating_add(p.length).min(bytes.len());
            Ok(
                json!({"revision":p.expected_revision,"offset":p.offset,"total_bytes":bytes.len(),"sha256":snapshot.content_hash,"data_base64":STANDARD.encode(&bytes[p.offset..end])}),
            )
        }
        _ => Err(DomainError {
            code: "CK-RUN-NO-METHOD".into(),
            component: "runtime".into(),
            summary: "Unknown notebook method".into(),
            action: "Use a supported domain method".into(),
            detail: None,
        }),
    }
}
