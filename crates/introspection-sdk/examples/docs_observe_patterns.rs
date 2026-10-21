//! Docs snippet for `observe/patterns`: send the user's feedback on an answer
//! and a business event, both attributed to the user.
//!
//! Needs the `otel` feature for `IntrospectionLogs`. Run with:
//! ```sh
//! INTROSPECTION_TOKEN=intro_xxx TASK_ID=<task-id> \
//!   cargo run --example docs_observe_patterns --features otel
//! ```

#[cfg(feature = "otel")]
mod example {
    use std::error::Error;

    use introspection_sdk::{
        ClientConfig, FeedbackOptions, IntrospectionClient, IntrospectionLogs, TrackOptions,
    };

    pub async fn run() -> Result<(), Box<dyn Error>> {
        dotenvy::dotenv().ok();
        let client = IntrospectionClient::new(ClientConfig::default())?;
        let task = client.tasks().get(&std::env::var("TASK_ID")?).await?;
        let user_id = std::env::var("APP_USER_ID").unwrap_or_else(|_| "user_123".into());
        let response_id = task
            .metadata
            .as_ref()
            .and_then(|m| m.get("last_response"))
            .and_then(|r| r["response_id"].as_str())
            .unwrap_or_default();

        // #region docs:observe/patterns/feedback-events
        let logs = IntrospectionLogs::builder()
            .service_name("support-api")
            .build()?;

        let _user = logs.set_user_id(&user_id); // attributes both events while held
        logs.feedback(
            "thumbs_down",
            FeedbackOptions::new()
                .with_conversation_id(task.id.to_string())
                .with_previous_response_id(response_id) // the answer being rated
                .with_comments("Applied the wrong refund rule"),
        );
        logs.track(
            "case_closed",
            Some(
                TrackOptions::new()
                    .with_property("value", 1)
                    .with_property("plan", "pro"),
            ),
        );
        // #endregion docs:observe/patterns/feedback-events

        logs.shutdown()?;
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
    eprintln!("this example needs the `otel` feature: cargo run --example docs_observe_patterns --features otel");
}
