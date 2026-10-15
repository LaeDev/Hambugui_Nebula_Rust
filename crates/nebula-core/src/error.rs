#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Invalid(String),
    #[error("unsafe path: {0}")]
    UnsafePath(String),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    /// An empty `url` would print as a dangling `HTTP : …`, so the message
    /// stands on its own then.
    #[error("{}", if url.is_empty() { message.clone() } else { format!("HTTP {url}: {message}") })]
    Http { url: String, message: String },
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("{tool} exited with {code:?}")]
    Tool { tool: String, code: Option<i32> },
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn io(context: impl Into<String>) -> impl FnOnce(std::io::Error) -> Error {
    let context = context.into();
    move |source| Error::Io { context, source }
}
