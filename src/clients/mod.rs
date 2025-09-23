use momento::MomentoError;
use momento::MomentoErrorCode;

pub mod common;

pub mod cache;
pub mod leaderboard;
pub mod oltp;
pub mod ping;
pub mod pubsub;
pub mod store;

use xxhash_rust::xxh3::xxh3_64;

pub fn compute_hash(to_hash: &[u8]) -> u64 {
    xxh3_64(to_hash)
}

pub enum ResponseError {
    /// Some exception while reading the response
    Exception,
    /// A timeout while awaiting the response
    Timeout,
    /// Some backends may have rate limits
    Ratelimited,
    /// Some backends may have their own timeout
    BackendTimeout,
}

impl From<MomentoError> for ResponseError {
    fn from(other: MomentoError) -> Self {
        match other.error_code {
            MomentoErrorCode::LimitExceededError { .. } => ResponseError::Ratelimited,
            MomentoErrorCode::TimeoutError { .. } => ResponseError::BackendTimeout,
            _ => ResponseError::Exception,
        }
    }
}
