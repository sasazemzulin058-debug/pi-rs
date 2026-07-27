pub mod anthropic;
pub mod google;
pub mod openai;
pub mod openai_responses;

use async_trait::async_trait;

use crate::error::Result;
use crate::stream::AssistantMessageEventStream;
use crate::types::{Context, Model, StreamOptions};

/// Provider implementation used by the default model dispatcher.
#[async_trait]
pub trait Provider: Send + Sync {
    async fn stream(
        &self,
        model: &Model,
        context: &Context,
        options: &StreamOptions,
    ) -> Result<AssistantMessageEventStream>;
}

/// Injection seam for selecting a provider implementation in the agent loop.
#[async_trait]
pub trait ProviderFactory: Send + Sync {
    async fn stream(
        &self,
        model: &Model,
        context: &Context,
        options: &StreamOptions,
    ) -> Result<AssistantMessageEventStream>;
}

/// Default provider factory preserving the existing model dispatch.
pub struct DefaultProviderFactory;

#[async_trait]
impl ProviderFactory for DefaultProviderFactory {
    async fn stream(
        &self,
        model: &Model,
        context: &Context,
        options: &StreamOptions,
    ) -> Result<AssistantMessageEventStream> {
        crate::stream_simple(model, context, options).await
    }
}

/// Fake provider factory that replays a fixed sequence of events on stream().
pub struct FakeProviderFactory {
    events: Vec<crate::types::AssistantMessageEvent>,
}

impl FakeProviderFactory {
    pub fn new(events: Vec<crate::types::AssistantMessageEvent>) -> Self {
        Self { events }
    }
}

#[async_trait]
impl ProviderFactory for FakeProviderFactory {
    async fn stream(
        &self,
        _model: &Model,
        _context: &Context,
        _options: &StreamOptions,
    ) -> Result<AssistantMessageEventStream> {
        let stream_events: Vec<Result<crate::types::AssistantMessageEvent>> =
            self.events.iter().cloned().map(Ok).collect();
        Ok(Box::pin(futures::stream::iter(stream_events)))
    }
}
