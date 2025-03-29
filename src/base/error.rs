use tokio::sync::oneshot;

//#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Cluster error: {error}")]
    ClusterError { error: String },

    #[error("Crypto error: {context}, {error}")]
    CryptoError {
        context: String,
        error: threshold_crypto::error::Error,
    },

    #[error("Oneshot recv error: {context}, {error}")]
    OneshotRecvError {
        context: String,
        error: oneshot::error::RecvError,
    },

    #[error("HTTP request error: {context}, {error}")]
    HttpRequestError { context: String, error: String },

    #[error("Oneshot recv error: {context}, {error}")]
    JsonError {
        context: String,
        error: serde_json::Error,
    },
}

impl Error {
    pub fn cluster_error(error: impl Into<String>) -> Self {
        Self::ClusterError {
            error: error.into(),
        }
    }

    pub fn crypto_error(
        context: impl Into<String>,
    ) -> impl FnOnce(threshold_crypto::error::Error) -> Self {
        move |error| Self::CryptoError {
            context: context.into(),
            error,
        }
    }

    pub fn oneshot_recv_error(
        context: impl Into<String>,
    ) -> impl FnOnce(oneshot::error::RecvError) -> Self {
        move |error| Self::OneshotRecvError {
            context: context.into(),
            error,
        }
    }

    pub fn http_request_error(context: impl Into<String>) -> impl FnOnce(reqwest::Error) -> Self {
        move |error| Self::HttpRequestError {
            context: context.into(),
            error: error.to_string(),
        }
    }

    pub fn json_error(context: impl Into<String>) -> impl FnOnce(serde_json::Error) -> Self {
        move |error| Self::JsonError {
            context: context.into(),
            error,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
