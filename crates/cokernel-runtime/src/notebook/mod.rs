//! Runtime-owned notebook documents. The worker never writes notebook files.
mod model;
mod output;
pub use model::{CellKind, Edit, MAX_NOTEBOOK_BYTES, Notebook, NotebookError};
pub use output::NotebookOutputs;

#[cfg(target_os = "linux")]
mod import;
#[cfg(target_os = "linux")]
pub use import::ImportTicket;

#[cfg(target_os = "linux")]
mod execution;
#[cfg(target_os = "linux")]
mod queue;
#[cfg(target_os = "linux")]
pub use queue::{DocumentQueue, DocumentTicket};
#[cfg(target_os = "linux")]
mod service;
#[cfg(target_os = "linux")]
pub mod view;
#[cfg(target_os = "linux")]
pub use execution::{
    DocumentExecutionState, DocumentOperation, NotebookRuntime, PreparedExecution,
};
#[cfg(target_os = "linux")]
mod storage;
#[cfg(target_os = "linux")]
pub use service::{DocumentSnapshot, DocumentState, NotebookService};
