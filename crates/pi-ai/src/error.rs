use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("missing API key for provider {0}")]
    MissingApiKey(String),

    #[error("unsupported provider/api: {0}")]
    UnsupportedProvider(String),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("invalid response from provider: {0}")]
    InvalidResponse(String),

    #[error("provider returned an error (status {status}): {body}")]
    ProviderError { status: u16, body: String },

    #[error("request cancelled")]
    Cancelled,

    #[error("retried {attempts} times, last error: {source}")]
    RetryExhausted { attempts: u32, source: Box<Error> },

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

pub fn is_context_overflow_message(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("context_length_exceeded")
        || lower.contains("context_window_exceeded")
        || lower.contains("maximum context length")
        || lower.contains("prompt is too long")
        || lower.contains("token limit exceeded")
}

impl Error {
    pub fn is_context_overflow(&self) -> bool {
        match self {
            Error::ProviderError { body, .. } => is_context_overflow_message(body),
            Error::RetryExhausted { source, .. } => source.is_context_overflow(),
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
