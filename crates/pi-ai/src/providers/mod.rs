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
