//! Docs snippets for `patterns/multi-tenant`: name the end user on every
//! task, and pass per-task configuration.
//!
//! Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx cargo run --example docs_multi_tenant
//! ```

use std::{collections::HashMap, error::Error};

use introspection_sdk::{
    ClientConfig, IntrospectionClient, RunRequest, RunnerIdentity, TaskCreate,
};
use serde_json::json;

/// Stands in for the user of your server session.
struct User {
    id: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let client = IntrospectionClient::new(ClientConfig::default())?;
    let user = User {
        id: "user-4821".into(),
    };
    let prompt = "Where is my order?";

    // #region docs:patterns/multi-tenant/end-user
    let runner = client
        .runtime("support-agent")
        .await?
        .run(RunRequest {
            identity: Some(RunnerIdentity {
                user_id: Some(user.id),
                tags: Some(vec!["customer:acme".into()]),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await?;
    // #endregion docs:patterns/multi-tenant/end-user

    // #region docs:patterns/multi-tenant/per-task-config
    let handle = runner
        .tasks()
        .start(&TaskCreate {
            prompt: Some(prompt.into()),
            metadata: Some(HashMap::from([
                ("customer".into(), json!("acme")),
                ("playbook".into(), json!("acme-v3")),
                ("region".into(), json!("eu")),
            ])),
            tags: Some(vec!["customer:acme".into()]),
            ..Default::default()
        })
        .await?;
    // #endregion docs:patterns/multi-tenant/per-task-config

    println!("{}", handle.text().await?);
    Ok(())
}
