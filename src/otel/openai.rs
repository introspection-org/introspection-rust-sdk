//! Helpers for instrumenting [`async-openai`](https://docs.rs/async-openai) calls
//! with the Observation API.
//!
//! Requires the `openai` feature.
//!
//! # Example
//!
//! ```rust,no_run
//! use async_openai::config::OpenAIConfig;
//! use async_openai::types::chat::{
//!     ChatCompletionRequestMessage, ChatCompletionRequestUserMessage,
//!     CreateChatCompletionRequest,
//! };
//! use async_openai::Client;
//! use introspection_sdk::otel::openai::traced_chat_completion;
//! use opentelemetry::trace::TracerProvider;
//! use opentelemetry_sdk::trace::SdkTracerProvider;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let provider = SdkTracerProvider::builder().build();
//! let tracer = provider.tracer("my-app");
//! let client = Client::with_config(OpenAIConfig::default());
//!
//! let request = CreateChatCompletionRequest {
//!     model: "gpt-4o-mini".to_string(),
//!     messages: vec![ChatCompletionRequestMessage::User(
//!         ChatCompletionRequestUserMessage {
//!             content: "Hello!".into(),
//!             ..Default::default()
//!         },
//!     )],
//!     ..Default::default()
//! };
//!
//! let response = traced_chat_completion(&tracer, &client, request).await?;
//! # Ok(())
//! # }
//! ```

use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};

use async_openai::config::OpenAIConfig;
use async_openai::error::OpenAIError;
use async_openai::types::chat::{
    ChatCompletionMessageToolCalls, ChatCompletionRequestAssistantMessageContent,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageContent,
    ChatCompletionRequestSystemMessageContentPart, ChatCompletionRequestToolMessageContent,
    ChatCompletionRequestUserMessageContent, ChatCompletionRequestUserMessageContentPart,
    ChatCompletionResponseStream, ChatCompletionStreamOptions, ChatCompletionTools,
    CompletionUsage, CreateChatCompletionRequest, CreateChatCompletionResponse,
    CreateChatCompletionStreamResponse,
};
use async_openai::types::responses::{CreateResponse, OutputItem, Response as ResponsesResponse};
use async_openai::Client;
use futures::Stream;
use opentelemetry::trace::{Span, Tracer};
use opentelemetry::{KeyValue, StringValue, Value};

use crate::otel::messages::{
    ContentPart, InputMessage, OutputMessage, TextPart, ThinkingPart, ToolCallRequestPart,
    ToolCallResponsePart,
};
use crate::otel::observation::{GenerationUpdate, Observation, ObservationConfig};

/// Convert a slice of OpenAI request messages to typed [`InputMessage`] structs.
pub fn convert_request_messages(messages: &[ChatCompletionRequestMessage]) -> Vec<InputMessage> {
    messages
        .iter()
        .map(|msg| match msg {
            ChatCompletionRequestMessage::User(m) => {
                let content = match &m.content {
                    ChatCompletionRequestUserMessageContent::Text(text) => {
                        vec![ContentPart::Text(TextPart::new(text))]
                    }
                    ChatCompletionRequestUserMessageContent::Array(parts) => parts
                        .iter()
                        .filter_map(|p| match p {
                            ChatCompletionRequestUserMessageContentPart::Text(t) => {
                                Some(ContentPart::Text(TextPart::new(&t.text)))
                            }
                            _ => None,
                        })
                        .collect(),
                };
                InputMessage {
                    role: "user".to_string(),
                    parts: content,
                }
            }
            ChatCompletionRequestMessage::System(m) => {
                let parts = match &m.content {
                    ChatCompletionRequestSystemMessageContent::Text(text) => {
                        vec![ContentPart::Text(TextPart::new(text))]
                    }
                    ChatCompletionRequestSystemMessageContent::Array(parts) => parts
                        .iter()
                        .map(|p| match p {
                            ChatCompletionRequestSystemMessageContentPart::Text(t) => {
                                ContentPart::Text(TextPart::new(&t.text))
                            }
                        })
                        .collect(),
                };
                InputMessage {
                    role: "system".to_string(),
                    parts,
                }
            }
            ChatCompletionRequestMessage::Assistant(m) => {
                let mut parts = vec![];
                if let Some(c) = &m.content {
                    match c {
                        ChatCompletionRequestAssistantMessageContent::Text(text) => {
                            parts.push(ContentPart::Text(TextPart::new(text)));
                        }
                        ChatCompletionRequestAssistantMessageContent::Array(_) => {}
                    }
                }
                if let Some(tool_calls) = &m.tool_calls {
                    for tc in tool_calls {
                        if let ChatCompletionMessageToolCalls::Function(f) = tc {
                            parts.push(ContentPart::ToolCallRequest(ToolCallRequestPart::new(
                                &f.id,
                                &f.function.name,
                                &f.function.arguments,
                            )));
                        }
                    }
                }
                InputMessage {
                    role: "assistant".to_string(),
                    parts,
                }
            }
            ChatCompletionRequestMessage::Tool(m) => {
                let text = match &m.content {
                    ChatCompletionRequestToolMessageContent::Text(t) => t.clone(),
                    _ => String::new(),
                };
                InputMessage {
                    role: "tool".to_string(),
                    parts: vec![ContentPart::ToolCallResponse(ToolCallResponsePart::new(
                        &m.tool_call_id,
                        Some(text),
                    ))],
                }
            }
            _ => InputMessage {
                role: "unknown".to_string(),
                parts: vec![],
            },
        })
        .collect()
}

/// Convert OpenAI response choices to typed [`OutputMessage`] structs.
pub fn convert_response_choices(
    choices: &[async_openai::types::chat::ChatChoice],
) -> Vec<OutputMessage> {
    choices
        .iter()
        .map(|c| {
            let mut parts = vec![];
            let mut has_tool_calls = false;
            if let Some(text) = &c.message.content {
                parts.push(ContentPart::Text(TextPart::new(text)));
            }
            if let Some(tool_calls) = &c.message.tool_calls {
                has_tool_calls = !tool_calls.is_empty();
                for tc in tool_calls {
                    if let ChatCompletionMessageToolCalls::Function(f) = tc {
                        parts.push(ContentPart::ToolCallRequest(ToolCallRequestPart::new(
                            &f.id,
                            &f.function.name,
                            &f.function.arguments,
                        )));
                    }
                }
            }
            let finish_reason = if has_tool_calls {
                Some("tool-calls".to_string())
            } else {
                Some("stop".to_string())
            };
            OutputMessage {
                role: "assistant".to_string(),
                parts,
                finish_reason,
            }
        })
        .collect()
}

/// Wraps an OpenAI chat completion call with `tracing` spans carrying
/// gen_ai semantic convention attributes.
///
/// Use this when your spans need to flow through a tracing-based pipeline.
/// For direct OTel pipelines, use [`traced_chat_completion`].
pub async fn tracing_traced_chat_completion(
    client: &Client<OpenAIConfig>,
    request: CreateChatCompletionRequest,
) -> Result<CreateChatCompletionResponse, OpenAIError> {
    let span = tracing::info_span!(
        "chat",
        "gen_ai.system" = crate::otel::observation::infer_system(&request.model)
            .as_deref()
            .unwrap_or("unknown"),
        "gen_ai.operation.name" = "chat",
        "gen_ai.request.model" = request.model.as_str(),
        "gen_ai.response.model" = tracing::field::Empty,
        "gen_ai.response.id" = tracing::field::Empty,
        "gen_ai.usage.input_tokens" = tracing::field::Empty,
        "gen_ai.usage.output_tokens" = tracing::field::Empty,
    );
    let _guard = span.enter();

    let result = client.chat().create(request).await;

    match &result {
        Ok(response) => {
            tracing::Span::current().record("gen_ai.response.model", response.model.as_str());
            tracing::Span::current().record("gen_ai.response.id", response.id.as_str());
            if let Some(usage) = &response.usage {
                tracing::Span::current()
                    .record("gen_ai.usage.input_tokens", i64::from(usage.prompt_tokens));
                tracing::Span::current().record(
                    "gen_ai.usage.output_tokens",
                    i64::from(usage.completion_tokens),
                );
            }
        }
        Err(e) => {
            tracing::Span::current().record("otel.status_code", "ERROR");
            tracing::Span::current().record("otel.status_message", e.to_string().as_str());
        }
    }

    result
}

/// Wraps an OpenAI chat completion call with the Observation API.
///
/// Creates a generation span with gen_ai semantic convention attributes,
/// makes the API call, and records the response (model, id, usage, output).
///
/// Works with any tracer (SDK tracer, global tracer, etc.).
pub async fn traced_chat_completion<S: Span, T: Tracer<Span = S>>(
    tracer: &T,
    client: &Client<OpenAIConfig>,
    request: CreateChatCompletionRequest,
) -> Result<CreateChatCompletionResponse, OpenAIError> {
    let input_messages = convert_request_messages(&request.messages);
    let mut obs = Observation::start(
        tracer,
        ObservationConfig::generation("chat", &request.model).with_input(input_messages),
    );

    // Extract system instructions from system messages
    let sys_instructions: Vec<serde_json::Value> = request
        .messages
        .iter()
        .filter_map(|m| match m {
            ChatCompletionRequestMessage::System(s) => {
                let text = match &s.content {
                    ChatCompletionRequestSystemMessageContent::Text(t) => t.clone(),
                    ChatCompletionRequestSystemMessageContent::Array(parts) => parts
                        .iter()
                        .map(|p| match p {
                            ChatCompletionRequestSystemMessageContentPart::Text(t) => {
                                t.text.clone()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                Some(serde_json::json!({"type": "text", "content": text}))
            }
            _ => None,
        })
        .collect();
    if !sys_instructions.is_empty() {
        obs.set_attribute(KeyValue::new(
            crate::otel::types::attr::GEN_AI_SYSTEM_INSTRUCTIONS,
            serde_json::to_string(&sys_instructions).unwrap_or_default(),
        ));
    }

    // Extract tool definitions
    if let Some(ref tools) = request.tools {
        let tool_defs: Vec<serde_json::Value> = tools
            .iter()
            .filter_map(|t| match t {
                ChatCompletionTools::Function(f) => Some(serde_json::json!({
                    "name": f.function.name,
                    "description": f.function.description,
                    "parameters": f.function.parameters,
                })),
                _ => None,
            })
            .collect();
        obs.set_attribute(KeyValue::new(
            crate::otel::types::attr::GEN_AI_TOOL_DEFINITIONS,
            serde_json::to_string(&tool_defs).unwrap_or_default(),
        ));
    }

    // If this future is cancelled (dropped mid-await), `obs` drops before
    // either set_ok/set_error runs, so the span is left Unset — the correct
    // "caller abort" outcome for the non-streaming path, no RAII guard needed.
    let result = client.chat().create(request).await;

    match &result {
        Ok(response) => {
            let output_messages = convert_response_choices(&response.choices);

            let (input_tokens, output_tokens) = response
                .usage
                .as_ref()
                .map(|u| (i64::from(u.prompt_tokens), i64::from(u.completion_tokens)))
                .unwrap_or((0, 0));

            obs.update_generation(
                GenerationUpdate::new()
                    .with_response_model(&response.model)
                    .with_response_id(&response.id)
                    .with_output(output_messages)
                    .with_usage(input_tokens, output_tokens),
            );
            obs.set_ok();
        }
        Err(e) => {
            obs.set_error(e.to_string());
        }
    }

    result
}

/// A wrapper around [`ChatCompletionResponseStream`] that automatically
/// instruments the stream with an [`Observation`].
///
/// On each chunk, it accumulates content, captures response metadata (id, model),
/// and records token usage. When the stream completes (or the wrapper is dropped
/// early), the observation is finalized with the accumulated data.
///
/// Created by [`traced_chat_completion_stream`].
pub struct TracedStream<S: Span> {
    inner: ChatCompletionResponseStream,
    observation: Option<Observation<S>>,
    accumulated_content: String,
    response_id: Option<String>,
    response_model: Option<String>,
    final_usage: Option<CompletionUsage>,
    had_error: bool,
    finalized: bool,
    /// Set once the stream reaches a terminal state under our control: any
    /// choice carries a `finish_reason`, or the underlying stream yields
    /// `Poll::Ready(None)`. If the wrapper is dropped before this flips true,
    /// the consumer stopped driving the stream early — a caller abort.
    stream_completed: bool,
}

impl<S: Span> TracedStream<S> {
    fn finalize(&mut self) {
        if self.finalized {
            return;
        }
        self.finalized = true;

        if let Some(mut obs) = self.observation.take() {
            let mut update = GenerationUpdate::new()
                .with_output(vec![OutputMessage::assistant(&self.accumulated_content)]);

            if let Some(ref id) = self.response_id {
                update = update.with_response_id(id);
            }
            if let Some(ref model) = self.response_model {
                update = update.with_response_model(model);
            }
            if let Some(ref usage) = self.final_usage {
                update = update.with_usage(
                    i64::from(usage.prompt_tokens),
                    i64::from(usage.completion_tokens),
                );
            }

            obs.update_generation(update);

            if self.had_error {
                obs.set_error("stream encountered an error");
            } else if self.stream_completed {
                // Normal completion: a terminal finish_reason arrived or the
                // underlying stream ended cleanly.
                obs.set_ok();
            } else {
                // Caller abort: the wrapper was dropped before the model
                // finished (no terminal finish_reason, no Poll::Ready(None)).
                // Leave the span status Unset — this is a deliberate stop, not
                // a failure or a false success — and stamp the abort markers so
                // the read layer can exclude it from error/success counts.
                // Mirrors the JS SDK's caller-abort handling.
                obs.set_attribute(KeyValue::new(
                    crate::otel::types::attr::GEN_AI_RESPONSE_FINISH_REASONS,
                    Value::Array(vec![StringValue::from("aborted")].into()),
                ));
                obs.set_attribute(KeyValue::new(
                    crate::otel::types::attr::INTROSPECTION_TERMINATION_REASON,
                    "cancelled",
                ));
            }
        }
    }
}

// Safety: All fields are Unpin. `inner` is Pin<Box<dyn Stream + Send>> (Unpin),
// and all other fields are plain data / Option types.
impl<S: Span> Unpin for TracedStream<S> {}

impl<S: Span> Stream for TracedStream<S> {
    type Item = Result<CreateChatCompletionStreamResponse, OpenAIError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let poll = Pin::new(&mut this.inner).poll_next(cx);

        match &poll {
            Poll::Ready(Some(Ok(chunk))) => {
                // Capture response metadata from the first chunk
                if this.response_id.is_none() {
                    this.response_id = Some(chunk.id.clone());
                    this.response_model = Some(chunk.model.clone());
                }

                // Accumulate content deltas
                for choice in &chunk.choices {
                    if let Some(ref content) = choice.delta.content {
                        this.accumulated_content.push_str(content);
                    }
                    // A terminal finish_reason means the model finished this
                    // choice — the stream completed under our control.
                    if choice.finish_reason.is_some() {
                        this.stream_completed = true;
                    }
                }

                // Capture usage from the final chunk
                if let Some(ref usage) = chunk.usage {
                    this.final_usage = Some(usage.clone());
                }
            }
            Poll::Ready(Some(Err(_))) => {
                this.had_error = true;
            }
            Poll::Ready(None) => {
                // Underlying stream ended cleanly — normal completion.
                this.stream_completed = true;
                this.finalize();
            }
            Poll::Pending => {}
        }

        poll
    }
}

impl<S: Span> Drop for TracedStream<S> {
    fn drop(&mut self) {
        self.finalize();
    }
}

/// Wraps an OpenAI streaming chat completion call with the Observation API.
///
/// Creates a generation span, automatically sets `stream_options.include_usage = true`
/// so token usage is captured, and returns a [`TracedStream`] that accumulates
/// response data as chunks arrive. The observation is finalized when the stream
/// completes or is dropped.
///
/// # Example
///
/// ```rust,no_run
/// use async_openai::config::OpenAIConfig;
/// use async_openai::types::chat::{
///     ChatCompletionRequestMessage, ChatCompletionRequestUserMessage,
///     CreateChatCompletionRequest,
/// };
/// use async_openai::Client;
/// use futures::StreamExt;
/// use introspection_sdk::otel::openai::traced_chat_completion_stream;
/// use opentelemetry::trace::TracerProvider;
/// use opentelemetry_sdk::trace::SdkTracerProvider;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let provider = SdkTracerProvider::builder().build();
/// let tracer = provider.tracer("my-app");
/// let client = Client::with_config(OpenAIConfig::default());
///
/// let request = CreateChatCompletionRequest {
///     model: "gpt-4o-mini".to_string(),
///     messages: vec![ChatCompletionRequestMessage::User(
///         ChatCompletionRequestUserMessage {
///             content: "Hello!".into(),
///             ..Default::default()
///         },
///     )],
///     ..Default::default()
/// };
///
/// let mut stream = traced_chat_completion_stream(&tracer, &client, request).await?;
/// while let Some(result) = stream.next().await {
///     let chunk = result?;
///     for choice in &chunk.choices {
///         if let Some(ref content) = choice.delta.content {
///             print!("{content}");
///         }
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub async fn traced_chat_completion_stream<S: Span, T: Tracer<Span = S>>(
    tracer: &T,
    client: &Client<OpenAIConfig>,
    mut request: CreateChatCompletionRequest,
) -> Result<TracedStream<S>, OpenAIError> {
    // Ensure usage is included in the stream so we can capture it
    let mut opts = request
        .stream_options
        .unwrap_or(ChatCompletionStreamOptions {
            include_usage: None,
            include_obfuscation: None,
        });
    opts.include_usage = Some(true);
    request.stream_options = Some(opts);

    let input_messages = convert_request_messages(&request.messages);
    let obs = Observation::start(
        tracer,
        ObservationConfig::generation("chat", &request.model).with_input(input_messages),
    );

    let inner = client.chat().create_stream(request).await?;

    Ok(TracedStream {
        inner,
        observation: Some(obs),
        accumulated_content: String::new(),
        response_id: None,
        response_model: None,
        final_usage: None,
        had_error: false,
        finalized: false,
        stream_completed: false,
    })
}

/// Wraps an OpenAI Responses API call with the Observation API.
///
/// Creates a generation span with gen_ai semantic convention attributes,
/// makes the API call, and records the response. Unlike Chat Completions,
/// multi-turn is handled via `previous_response_id` so each turn only
/// sends new input — no duplicate messages.
pub async fn traced_responses_create<S: Span, T: Tracer<Span = S>>(
    tracer: &T,
    client: &Client<OpenAIConfig>,
    request: CreateResponse,
) -> Result<ResponsesResponse, OpenAIError> {
    let model = request.model.clone().unwrap_or_default();

    // Convert input to gen_ai.input.messages
    let input_messages = convert_responses_input(&request.input);
    let mut obs = Observation::start(
        tracer,
        ObservationConfig::generation("chat", &model).with_input(input_messages),
    );

    // System instructions
    if let Some(ref instructions) = request.instructions {
        let sys = serde_json::json!([{"type": "text", "content": instructions}]);
        obs.set_attribute(KeyValue::new(
            crate::otel::types::attr::GEN_AI_SYSTEM_INSTRUCTIONS,
            sys.to_string(),
        ));
    }

    // Tool definitions
    if let Some(ref tools) = request.tools {
        use async_openai::types::responses::Tool;
        let tool_defs: Vec<serde_json::Value> = tools
            .iter()
            .filter_map(|t| match t {
                Tool::Function(f) => Some(serde_json::json!({
                    "name": f.name,
                    "description": f.description,
                    "parameters": f.parameters,
                })),
                Tool::Mcp(mcp) => Some(serde_json::json!({
                    "name": format!("mcp/{}", mcp.server_label),
                    "description": mcp.server_description,
                })),
                Tool::WebSearch(_) | Tool::WebSearch20250826(_) => Some(serde_json::json!({
                    "name": "web_search",
                    "description": "Search the web for relevant information",
                })),
                _ => {
                    let v = serde_json::to_value(t).ok()?;
                    Some(serde_json::json!({
                        "name": v.get("type"),
                    }))
                }
            })
            .collect();
        if !tool_defs.is_empty() {
            obs.set_attribute(KeyValue::new(
                crate::otel::types::attr::GEN_AI_TOOL_DEFINITIONS,
                serde_json::to_string(&tool_defs).unwrap_or_default(),
            ));
        }
    }

    let result = client.responses().create(request).await;

    match &result {
        Ok(response) => {
            let output_messages = convert_responses_output(&response.output);
            let (input_tokens, output_tokens) = response
                .usage
                .as_ref()
                .map(|u| (i64::from(u.input_tokens), i64::from(u.output_tokens)))
                .unwrap_or((0, 0));

            obs.update_generation(
                GenerationUpdate::new()
                    .with_response_model(&response.model)
                    .with_response_id(&response.id)
                    .with_output(output_messages)
                    .with_usage(input_tokens, output_tokens),
            );
            obs.set_ok();
        }
        Err(e) => {
            obs.set_error(e.to_string());
        }
    }

    result
}

/// Convert Responses API input to typed InputMessage structs.
pub fn convert_responses_input(
    input: &async_openai::types::responses::InputParam,
) -> Vec<InputMessage> {
    use async_openai::types::responses::InputParam;
    match input {
        InputParam::Text(text) => vec![InputMessage::user(text.as_str())],
        InputParam::Items(items) => items
            .iter()
            .filter_map(|item| {
                let v = serde_json::to_value(item).ok()?;
                let typ = v.get("type")?.as_str()?;
                match typ {
                    "message" => {
                        let role = v.get("role")?.as_str().unwrap_or("user").to_string();
                        let content = v.get("content").and_then(|c| c.as_str())?;
                        Some(InputMessage {
                            role,
                            parts: vec![ContentPart::Text(TextPart::new(content))],
                        })
                    }
                    "function_call_output" => {
                        let call_id = v.get("call_id").and_then(|c| c.as_str())?.to_string();
                        let output = v.get("output").and_then(|o| o.as_str())?.to_string();
                        Some(InputMessage {
                            role: "tool".to_string(),
                            parts: vec![ContentPart::ToolCallResponse(ToolCallResponsePart::new(
                                call_id,
                                Some(output),
                            ))],
                        })
                    }
                    _ => None,
                }
            })
            .collect(),
    }
}

/// Convert Responses API output items to typed OutputMessage structs.
pub fn convert_responses_output(
    output: &[OutputItem],
) -> Vec<crate::otel::messages::OutputMessage> {
    let mut messages = vec![];
    let mut prefix_parts: Vec<ContentPart> = vec![];
    let mut pending_tool_calls: Vec<ContentPart> = vec![];
    let mut pending_web_search_id: Option<String> = None;

    for item in output {
        match item {
            OutputItem::Message(msg) => {
                // Flush any pending tool calls first
                if !pending_tool_calls.is_empty() {
                    messages.push(crate::otel::messages::OutputMessage {
                        role: "assistant".to_string(),
                        parts: std::mem::take(&mut pending_tool_calls),
                        finish_reason: Some("tool-calls".to_string()),
                    });
                }

                // Extract web search citations from message annotations
                if let Some(ws_id) = pending_web_search_id.take() {
                    let mut citations: Vec<String> = vec![];
                    for content in &msg.content {
                        let v = serde_json::to_value(content).unwrap_or_default();
                        if let Some(anns) = v.get("annotations").and_then(|a| a.as_array()) {
                            for ann in anns {
                                let title = ann.get("title").and_then(|t| t.as_str());
                                let url = ann.get("url").and_then(|u| u.as_str());
                                if let (Some(t), Some(u)) = (title, url) {
                                    citations.push(format!("{t}: {u}"));
                                }
                            }
                        }
                    }
                    let result = if citations.is_empty() {
                        "search completed".to_string()
                    } else {
                        citations.join("\n")
                    };
                    prefix_parts.push(ContentPart::ToolCallResponse(ToolCallResponsePart::new(
                        ws_id,
                        Some(result),
                    )));
                }

                let mut parts = std::mem::take(&mut prefix_parts);
                for content in &msg.content {
                    let v = serde_json::to_value(content).unwrap_or_default();
                    if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
                        parts.push(ContentPart::Text(TextPart::new(text)));
                    }
                }
                let finish_reason =
                    if msg.status == async_openai::types::responses::OutputStatus::Completed {
                        Some("stop".to_string())
                    } else {
                        None
                    };
                messages.push(crate::otel::messages::OutputMessage {
                    role: "assistant".to_string(),
                    parts,
                    finish_reason,
                });
            }
            OutputItem::FunctionCall(fc) => {
                // Accumulate tool calls — they'll be flushed as one message
                pending_tool_calls.push(ContentPart::ToolCallRequest(ToolCallRequestPart::new(
                    &fc.call_id,
                    &fc.name,
                    &fc.arguments,
                )));
            }
            OutputItem::Reasoning(r) => {
                let summary_text: String = r
                    .summary
                    .iter()
                    .filter_map(|s| {
                        let v = serde_json::to_value(s).ok()?;
                        v.get("text")?.as_str().map(|t| t.to_string())
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let content = if summary_text.is_empty() {
                    None
                } else {
                    Some(summary_text)
                };
                let signature = r.encrypted_content.clone();
                prefix_parts.push(ContentPart::Thinking(ThinkingPart::new(content, signature)));
            }
            OutputItem::WebSearchCall(ws) => {
                let id = ws.id.clone();
                let query = serde_json::to_value(&ws.action)
                    .ok()
                    .and_then(|v| v.get("query")?.as_str().map(|s| s.to_string()));
                let args = query.as_ref().map(|q| format!(r#"{{"query":"{q}"}}"#));
                prefix_parts.push(ContentPart::ToolCallRequest(ToolCallRequestPart::new(
                    &id,
                    "web_search",
                    args.as_deref().unwrap_or(""),
                )));
                // Citations will be extracted from the next message's annotations
                pending_web_search_id = Some(id);
            }
            OutputItem::McpCall(mcp) => {
                let tool_name = format!("{}/{}", mcp.server_label, mcp.name);
                prefix_parts.push(ContentPart::ToolCallRequest(ToolCallRequestPart::new(
                    &mcp.id,
                    &tool_name,
                    &mcp.arguments,
                )));
                let result = mcp
                    .error
                    .as_ref()
                    .map(|e| e.to_string())
                    .or_else(|| mcp.output.clone())
                    .unwrap_or_default();
                prefix_parts.push(ContentPart::ToolCallResponse(ToolCallResponsePart::new(
                    &mcp.id,
                    Some(result),
                )));
            }
            _ => {} // Other output items — skip
        }
    }

    // Flush any remaining pending tool calls
    if !pending_tool_calls.is_empty() {
        messages.push(crate::otel::messages::OutputMessage {
            role: "assistant".to_string(),
            parts: pending_tool_calls,
            finish_reason: Some("tool-calls".to_string()),
        });
    }

    // If we have prefix parts but no message, wrap them
    if !prefix_parts.is_empty() {
        messages.push(crate::otel::messages::OutputMessage {
            role: "assistant".to_string(),
            parts: prefix_parts,
            finish_reason: None,
        });
    }

    messages
}

#[cfg(all(test, feature = "openai"))]
mod tests {
    use super::*;
    use crate::otel::testing::setup_test_provider;
    use crate::otel::types::attr;
    use futures::StreamExt;
    use opentelemetry::trace::{Status, TracerProvider};

    /// Parse a single SSE chunk JSON into a stream response.
    fn chunk(json: &str) -> CreateChatCompletionStreamResponse {
        serde_json::from_str(json).expect("valid chunk fixture")
    }

    /// Build a [`TracedStream`] over a fixed set of chunks, mirroring what
    /// `traced_chat_completion_stream` produces but without a network round-trip.
    fn traced_stream_over<S: Span>(
        obs: Observation<S>,
        chunks: Vec<CreateChatCompletionStreamResponse>,
    ) -> TracedStream<S> {
        let inner: ChatCompletionResponseStream =
            Box::pin(futures::stream::iter(chunks.into_iter().map(Ok)));
        TracedStream {
            inner,
            observation: Some(obs),
            accumulated_content: String::new(),
            response_id: None,
            response_model: None,
            final_usage: None,
            had_error: false,
            finalized: false,
            stream_completed: false,
        }
    }

    fn attr_value<'a>(
        span: &'a opentelemetry_sdk::trace::SpanData,
        key: &str,
    ) -> Option<&'a Value> {
        span.attributes
            .iter()
            .find(|kv| kv.key.as_str() == key)
            .map(|kv| &kv.value)
    }

    // A stream dropped before any terminal finish_reason (and before
    // Poll::Ready(None)) is a caller abort: status stays Unset and the abort
    // markers are stamped.
    #[tokio::test]
    async fn dropped_stream_is_annotated_as_cancelled() {
        let (provider, exporter) = setup_test_provider();
        let tracer = provider.tracer("test");

        {
            let obs = Observation::start(
                &tracer,
                ObservationConfig::generation("chat", "gpt-4o-mini"),
            );
            let mut stream = traced_stream_over(
                obs,
                vec![
                    chunk(
                        r#"{"id":"chatcmpl-abort","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"role":"assistant","content":"Hel"},"finish_reason":null}],"usage":null}"#,
                    ),
                    chunk(
                        r#"{"id":"chatcmpl-abort","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"content":"lo"},"finish_reason":null}],"usage":null}"#,
                    ),
                ],
            );

            // Consume the two content chunks but never reach Poll::Ready(None):
            // the consumer stopped driving the stream early.
            assert!(stream.next().await.is_some());
            assert!(stream.next().await.is_some());
            // stream dropped here → finalize() takes the cancel branch.
        }

        let spans = exporter.get_finished_spans().unwrap();
        assert_eq!(spans.len(), 1);
        let span = &spans[0];

        assert_eq!(span.status, Status::Unset, "aborted span must stay Unset");

        let finish_reasons = attr_value(span, attr::GEN_AI_RESPONSE_FINISH_REASONS)
            .expect("finish_reasons attribute present");
        assert_eq!(
            *finish_reasons,
            Value::Array(vec![StringValue::from("aborted")].into()),
        );

        let termination = attr_value(span, attr::INTROSPECTION_TERMINATION_REASON)
            .expect("termination_reason attribute present");
        assert_eq!(*termination, Value::String(StringValue::from("cancelled")));

        provider.shutdown().unwrap();
    }

    // A stream that reaches a terminal finish_reason and drains to
    // Poll::Ready(None) completes normally: status OK, no abort markers.
    #[tokio::test]
    async fn completed_stream_stays_ok_without_termination_marker() {
        let (provider, exporter) = setup_test_provider();
        let tracer = provider.tracer("test");

        {
            let obs = Observation::start(
                &tracer,
                ObservationConfig::generation("chat", "gpt-4o-mini"),
            );
            let mut stream = traced_stream_over(
                obs,
                vec![
                    chunk(
                        r#"{"id":"chatcmpl-ok","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}],"usage":null}"#,
                    ),
                    chunk(
                        r#"{"id":"chatcmpl-ok","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4o-mini","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":null}"#,
                    ),
                ],
            );

            // Drain fully, reaching Poll::Ready(None).
            while stream.next().await.is_some() {}
        }

        let spans = exporter.get_finished_spans().unwrap();
        assert_eq!(spans.len(), 1);
        let span = &spans[0];

        assert_eq!(span.status, Status::Ok, "completed span must be OK");
        assert!(
            attr_value(span, attr::INTROSPECTION_TERMINATION_REASON).is_none(),
            "completed span must not carry a termination_reason",
        );
        assert!(
            attr_value(span, attr::GEN_AI_RESPONSE_FINISH_REASONS).is_none(),
            "completed span must not carry the aborted finish_reasons marker",
        );

        provider.shutdown().unwrap();
    }
}
