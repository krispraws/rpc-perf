/// Removes a key from the cache.
use super::*;
use crate::clients::compute_hash;

impl From<&workload::client::Delete> for RequestWithValidatorAndHash {
    fn from(other: &workload::client::Delete) -> Self {
        DELETE.increment();
        RequestWithValidatorAndHash {
            request: Request::delete((*other.key).to_owned().into_boxed_slice(), false),
            validator: Box::new(validate_response),
            hash: compute_hash(&other.key),
        }
    }
}

pub fn validate_response(response: Response) -> std::result::Result<(), ()> {
    match response {
        Response::Deleted(_) => {
            DELETE_DELETED.increment();
            Ok(())
        }
        Response::NotFound(_) => {
            DELETE_NOT_FOUND.increment();
            Ok(())
        }
        _ => {
            DELETE_EX.increment();
            Err(())
        }
    }
}
