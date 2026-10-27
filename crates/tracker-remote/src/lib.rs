//! Async HTTP client for a remote tracker server.

mod application;
mod transport;

use reqwest::StatusCode;

pub use application::{InactiveTaskPreview, RemoteApplication, RemoteFailureKind};

/// A connection or wire failure. Semantic command errors use
/// `ApplicationError` in operation results.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RemoteError {
    #[error("invalid tracker endpoint: {0}")]
    InvalidEndpoint(&'static str),
    #[error("tracker server is unavailable: {0}")]
    Unavailable(String),
    #[error("tracker server returned invalid data: {0}")]
    Protocol(String),
    #[error("tracker server returned HTTP {status}")]
    Http { status: StatusCode, body: Vec<u8> },
}

impl RemoteError {
    /// Whether a connection or server failure prevented a confirmed result.
    pub fn is_unavailable(&self) -> bool {
        match self {
            Self::Unavailable(_) => true,
            Self::Http { status, .. } => {
                status.is_server_error() || *status == StatusCode::REQUEST_TIMEOUT
            }
            Self::InvalidEndpoint(_) | Self::Protocol(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::RemoteError;

    fn http(status: StatusCode) -> RemoteError {
        RemoteError::Http {
            status,
            body: Vec::new(),
        }
    }

    #[test]
    fn unavailable_classification_distinguishes_connection_and_protocol_failures() {
        assert!(RemoteError::Unavailable("offline".into()).is_unavailable());
        for status in [
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::REQUEST_TIMEOUT,
        ] {
            assert!(http(status).is_unavailable(), "{status}");
        }
        for status in [StatusCode::BAD_REQUEST, StatusCode::CONFLICT] {
            assert!(!http(status).is_unavailable(), "{status}");
        }
        assert!(!RemoteError::Protocol("version mismatch".into()).is_unavailable());
        assert!(!RemoteError::InvalidEndpoint("invalid URL").is_unavailable());
    }
}
