//! Docs snippets for `patterns/add-to-your-app`: open a runner for the
//! signed-in user, start and stream a task, reconnect after a reload, and
//! record the user's feedback on the answer.
//!
//! Needs the `otel` feature for `IntrospectionLogs`. Run with:
//! ```sh
//! INTROSPECTION_SERVICE_ACCOUNT_CLIENT_ID=intro_app_xxx \
//! INTROSPECTION_SERVICE_ACCOUNT_CLIENT_SECRET=intro_sk_xxx \
//! INTROSPECTION_PROJECT=<project> INTROSPECTION_API_KEY=intro_xxx \
//!   cargo run --example docs_add_to_your_app --features otel
//! ```

#[cfg(feature = "otel")]
mod example {
    use std::{collections::HashMap, error::Error};

    use futures::StreamExt;
    use introspection_sdk::{
        AgUiEvent, FeedbackOptions, IntrospectionClient, IntrospectionLogs, RunRequest, Runner,
        RunnerIdentity, ServiceAccountTokenParams, TaskCreate,
    };

    pub async fn run() -> Result<(), Box<dyn Error>> {
        dotenvy::dotenv().ok();
        // The signed-in user, from your server session.
        let user_id = std::env::var("APP_USER_ID").unwrap_or_else(|_| "user_123".into());
        let order_id = "1842";

        // #region docs:patterns/add-to-your-app/open-runner
        let credentials = ServiceAccountTokenParams::builder()
            .client_id(std::env::var("INTROSPECTION_SERVICE_ACCOUNT_CLIENT_ID")?)
            .client_secret(std::env::var(
                "INTROSPECTION_SERVICE_ACCOUNT_CLIENT_SECRET",
            )?)
            .project(std::env::var("INTROSPECTION_PROJECT")?)
            .build()?;

        let client = IntrospectionClient::from_service_account(credentials, None).await?;
        // An API key: this client keeps one token for life.
        let outcomes = IntrospectionLogs::builder()
            .token(std::env::var("INTROSPECTION_API_KEY")?)
            .service_name("refund-app")
            .build()?;

        let runner = client
            .runtime("refund-agent")
            .await?
            .run(RunRequest {
                identity: Some(RunnerIdentity {
                    user_id: Some(user_id.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .await?;
        // #endregion docs:patterns/add-to-your-app/open-runner

        // #region docs:patterns/add-to-your-app/start-task
        // Your app's code: store the task next to the order and forward events to the browser.
        struct AgentBinding {
            task_id: String,
            user_id: String,
        }
        async fn save_agent_binding(order_id: &str, binding: &AgentBinding) {
            println!(
                "order {order_id} -> task {} (user {})",
                binding.task_id, binding.user_id
            );
        }
        async fn send_event_to_browser(event: AgUiEvent) {
            println!("{event:?}");
        }

        let handle = runner
            .tasks()
            .start(&TaskCreate {
                prompt: Some(
                    "Review order 1842 against the refund policy and draft the next response."
                        .into(),
                ),
                metadata: Some(HashMap::from([("order_id".into(), "1842".into())])),
                ..Default::default()
            })
            .await?;
        let binding = AgentBinding {
            task_id: handle.run.task_id.to_string(),
            user_id: user_id.clone(),
        };
        save_agent_binding(order_id, &binding).await;

        let stream = handle.stream().await?;
        tokio::pin!(stream);
        while let Some(event) = stream.next().await {
            send_event_to_browser(event?).await;
        }
        // #endregion docs:patterns/add-to-your-app/start-task

        reconnect(&runner, &binding.task_id).await?;

        record_feedback(&runner, &binding.task_id, &outcomes).await?;

        runner.close();
        outcomes.shutdown()?;
        client.shutdown()?;
        Ok(())
    }

    /// After a page reload: find the task's run and stream it again.
    async fn reconnect(runner: &Runner, task_id: &str) -> Result<(), Box<dyn Error>> {
        // #region docs:patterns/add-to-your-app/reconnect
        // Your app's code: forward each event to the browser.
        async fn send_event_to_browser(event: AgUiEvent) {
            println!("{event:?}");
        }

        let task = runner.tasks().get(task_id).await?;
        let metadata = task.metadata.unwrap_or_default();
        let run_id = metadata
            .get("active_run_id")
            .or_else(|| metadata.get("latest_run_id"))
            .and_then(|id| id.as_str())
            .ok_or("task has no run")?;
        let stream = runner.tasks().runs.stream(task_id, run_id).await?;
        tokio::pin!(stream);
        while let Some(event) = stream.next().await {
            send_event_to_browser(event?).await;
        }
        // #endregion docs:patterns/add-to-your-app/reconnect
        Ok(())
    }

    /// Record the user's thumbs-down on the agent's last answer.
    async fn record_feedback(
        runner: &Runner,
        task_id: &str,
        outcomes: &IntrospectionLogs,
    ) -> Result<(), Box<dyn Error>> {
        // #region docs:patterns/add-to-your-app/feedback
        let task = runner.tasks().get(task_id).await?;
        let response_id = task
            .metadata
            .as_ref()
            .and_then(|m| m.get("last_response"))
            .and_then(|r| r["response_id"].as_str());
        let mut options = FeedbackOptions::new()
            .with_conversation_id(task_id)
            .with_comments("Missed the final-sale rule");
        if let Some(id) = response_id {
            options = options.with_previous_response_id(id);
        }
        outcomes.feedback("thumbs_down", options);
        outcomes.flush()?;
        // #endregion docs:patterns/add-to-your-app/feedback
        Ok(())
    }
}

#[cfg(feature = "otel")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    example::run().await
}

#[cfg(not(feature = "otel"))]
fn main() {
    eprintln!("this example needs the `otel` feature: cargo run --example docs_add_to_your_app --features otel");
}
