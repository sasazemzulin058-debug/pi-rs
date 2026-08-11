pub mod anthropic;
pub mod google;
pub mod openai;
pub mod openai_responses;

use async_trait::async_trait;

use futures::StreamExt;

use crate::error::{Error, Result};
use crate::stream::AssistantMessageEventStream;
use crate::types::{AssistantMessage, AssistantMessageEvent, Context, Model, StreamOptions};

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

    /// Collect a full stream into a single AssistantMessage result.
    async fn complete(
        &self,
        model: &Model,
        context: &Context,
        options: &StreamOptions,
    ) -> Result<AssistantMessage> {
        let mut stream = self.stream(model, context, options).await?;
        while let Some(event_res) = stream.next().await {
            let event = event_res?;
            match event {
                AssistantMessageEvent::Done { message, .. } => return Ok(message),
                AssistantMessageEvent::Error { error, .. } => return Ok(error),
                _ => {}
            }
        }
        Err(Error::InvalidResponse(
            "Stream ended without Done event".into(),
        ))
    }
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
        options: &StreamOptions,
    ) -> Result<AssistantMessageEventStream> {
        let events = self.events.clone();
        let cancel = options.cancel.clone();
        let s = async_stream::stream! {
            for event in events {
                if let Some(ref c) = cancel {
                    if c.is_cancelled() {
                        yield Err(crate::error::Error::Cancelled);
                        return;
                    }
                }
                yield Ok(event);
            }
        };
        Ok(Box::pin(s))
    }
}
