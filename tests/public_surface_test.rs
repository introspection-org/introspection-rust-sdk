//! Every type reachable as a *public field* of an exported type must itself be
//! nameable from the crate root.
//!
//! `TaskCreate` is re-exported from the crate root, so `TaskCreate { files, .. }`
//! is visible to any caller — but naming the value to put in `files` needs
//! `TaskFileRef`, and that was reachable only through `api::schemas::`. A caller
//! following the documented `use introspection_sdk::X` form could read the field
//! and not construct it.
//!
//! These are compile-time assertions: the paths below either resolve or the
//! test target fails to build.

use introspection_sdk::{RuntimeLlmMode, TaskCreate, TaskFileRef, TaskRepoRequest, TaskRunCreate};

#[test]
fn a_task_file_ref_is_nameable_from_the_crate_root() {
    let file = TaskFileRef {
        id: "file_123".into(),
        name: Some("specs/senior-role.pdf".into()),
        ..Default::default()
    };

    let create = TaskCreate {
        prompt: Some("Summarize the attached notes.".into()),
        files: Some(vec![file.clone()]),
        ..Default::default()
    };
    let run = TaskRunCreate {
        files: Some(vec![file]),
        ..Default::default()
    };

    assert_eq!(create.files.unwrap().len(), 1);
    assert_eq!(run.files.unwrap().len(), 1);
}

#[test]
fn a_task_repo_request_is_nameable_from_the_crate_root() {
    let create = TaskCreate {
        repositories: Some(vec![TaskRepoRequest::default()]),
        ..Default::default()
    };

    assert_eq!(create.repositories.unwrap().len(), 1);
}

#[test]
fn a_runtime_llm_mode_is_nameable_from_the_crate_root() {
    // Naming the type is the assertion; the comparison keeps it a live value.
    let mode = RuntimeLlmMode::default();
    assert_eq!(mode, RuntimeLlmMode::default());
}
