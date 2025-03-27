#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("Cluster error: {error}")]
    ClusterError { error: String },

    #[error("Crypto error: {error}")]
    CryptoError {
        context: String,
        error: threshold_crypto::error::Error,
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
}

pub type Result<T> = std::result::Result<T, Error>;
