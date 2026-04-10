//! # Introspection SDK for Rust
//!
//! A Rust client for tracking events and feedback using OTLP Logs.
//! Uses native OpenTelemetry libraries for batching, export, and baggage propagation.
//!
//! This SDK provides methods for:
//! - **Tracking events** - Custom analytics events with properties
//! - **Identifying users** - Associate events with user identities and traits
//! - **Collecting feedback** - Track user feedback on AI responses
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use introspection_sdk::{IntrospectionClient, ClientConfig, FeedbackOptions, TrackOptions};
//!
//! // Initialize the client
//! let client = IntrospectionClient::new(
//!     ClientConfig::with_token("your-token")
//! ).unwrap();
//!
//! // Track a custom event
//! client.track(
//!     "Button Clicked",
//!     Some(TrackOptions::new().with_property("button_id", "submit")),
//! );
//!
//! // Use baggage context for identity and conversation
//! {
//!     let _user = client.set_user_id("user_123");
//!     let _conv = client.set_conversation_id("conv_456");
//!
//!     // Events within this scope have user and conversation context
//!     client.feedback(
//!         "thumbs_up",
//!         FeedbackOptions::new().with_comments("Great response!"),
//!     );
//! } // Context automatically cleared when guards are dropped
//!
//! // Shutdown gracefully
//! client.shutdown().unwrap();
//! ```
//!
//! ## Configuration
//!
//! The client can be configured via the builder pattern or environment variables:
//!
//! ```rust,no_run
//! use introspection_sdk::{AdvancedOptions, IntrospectionClient, ClientConfig};
//!
//! // Using builder
//! let client = IntrospectionClient::new(
//!     ClientConfig::builder()
//!         .token("your-token")
//!         .service_name("my-service")
//!         .build()
//!         .unwrap()
//! ).unwrap();
//!
//! // With advanced options (custom base URL)
//! let client = IntrospectionClient::new(
//!     ClientConfig::builder()
//!         .token("your-token")
//!         .advanced(AdvancedOptions {
//!             base_url: Some("https://api.nuraline.ai".to_string()),
//!             ..Default::default()
//!         })
//!         .build()
//!         .unwrap()
//! ).unwrap();
//!
//! // Or use environment variables:
//! // - INTROSPECTION_TOKEN
//! // - INTROSPECTION_SERVICE_NAME
//! // - INTROSPECTION_BASE_URL
//! let client = IntrospectionClient::new(ClientConfig::default()).unwrap();
//! ```
//!
//! ## Context Management with Baggage
//!
//! Context is managed via OpenTelemetry baggage, which propagates across distributed systems.
//! Use the `set_*` methods to get guards that automatically clean up when dropped:
//!
//! ```rust,no_run
//! use introspection_sdk::{IntrospectionClient, ClientConfig};
//!
//! let client = IntrospectionClient::new(ClientConfig::default()).unwrap();
//!
//! // Guards automatically clear context when dropped (like Python's context managers)
//! {
//!     let _user_guard = client.set_user_id("user_123");
//!     let _conv_guard = client.set_conversation_id("conv_456");
//!     let _agent_guard = client.set_agent("support-bot", Some("agent_789"));
//!
//!     // All events here inherit the baggage context
//!     client.track("Message Sent", None);
//!     client.feedback("thumbs_up", Default::default());
//! }
//! // Context is cleared here
//! ```

pub mod client;
pub mod messages;
pub mod observation;
#[cfg(feature = "openai")]
pub mod openai;
#[cfg(feature = "openai")]
pub use openai::TracedStream;
pub mod span_processor;
#[cfg(any(feature = "testing", test))]
pub mod testing;
pub mod types;

// Re-export main types for convenience
pub use client::{BaggageGuard, IntrospectionClient, IntrospectionError, Result, VERSION};
pub use messages::{
    ContentPart, InputMessage, OutputMessage, TextPart, ThinkingPart, ToolCallRequestPart,
    ToolCallResponsePart,
};
pub use observation::{GenerationUpdate, Observation, ObservationConfig, ObservationType, Usage};
pub use span_processor::{
    IntrospectionSpanProcessor, SpanProcessorConfig, SpanProcessorConfigBuilder,
    SpanProcessorError, SpanProcessorResult,
};
pub use types::{
    AdvancedOptions, ClientConfig, ClientConfigBuilder, FeedbackOptions, IdentifyOptions,
    PropertyValue, TrackOptions,
};
