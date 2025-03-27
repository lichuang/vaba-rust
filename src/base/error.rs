use tokio::sync::oneshot;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
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
}

pub type Result<T> = std::result::Result<T, Error>;
