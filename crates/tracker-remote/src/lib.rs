//! Async HTTP client for a remote tracker server.

mod application;
mod transport;

use reqwest::StatusCode;

pub use application::{RemoteApplication, RemoteFailureKind};
pub use tracker_protocol::InactiveTaskPreviewDto;

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
            Self::Http { status, .. } => transport::is_unavailable_status(*status),
            Self::InvalidEndpoint(_) | Self::Protocol(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::RemoteError;

    #[test]
    fn unavailable_classification_distinguishes_connection_and_protocol_failures() {
        assert!(RemoteError::Unavailable("offline".into()).is_unavailable());
        assert!(
            RemoteError::Http {
                status: StatusCode::SERVICE_UNAVAILABLE,
                body: Vec::new(),
            }
            .is_unavailable()
        );
        assert!(
            !RemoteError::Http {
                status: StatusCode::CONFLICT,
                body: Vec::new(),
            }
            .is_unavailable()
        );
        assert!(!RemoteError::Protocol("version mismatch".into()).is_unavailable());
        assert!(!RemoteError::InvalidEndpoint("invalid URL").is_unavailable());
    }
}
