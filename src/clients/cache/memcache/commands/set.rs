// Sets a key-value pair in the cache.

use super::*;
use crate::clients::compute_hash;

impl From<&workload::client::Set> for RequestWithValidatorAndHash {
    fn from(other: &workload::client::Set) -> Self {
        SET.increment();
        RequestWithValidatorAndHash {
            request: Request::set(
                (*other.key).to_owned().into_boxed_slice(),
                (*other.value).to_owned().into_boxed_slice(),
                0,
                Ttl::none(),
                false,
            ),
            validator: Box::new(validate_response),
            hash: compute_hash(&other.key),
        }
    }
}

pub fn validate_response(response: Response) -> std::result::Result<(), ()> {
    match response {
        Response::Stored(_) => {
            SET_STORED.increment();
            Ok(())
        }
        Response::NotStored(_) => {
            SET_NOT_STORED.increment();
            Ok(())
        }
        _ => {
            SET_EX.increment();
            Err(())
        }
    }
}
