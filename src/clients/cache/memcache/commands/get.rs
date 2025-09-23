/// Retrieve the value of a key in the cache.
use super::*;
use crate::clients::compute_hash;

impl From<&workload::client::Get> for RequestWithValidatorAndHash {
    fn from(other: &workload::client::Get) -> Self {
        GET.increment();
        RequestWithValidatorAndHash {
            request: Request::get(
                vec![(*other.key).to_owned().into_boxed_slice()].into_boxed_slice(),
            ),
            validator: Box::new(validate_response),
            hash: compute_hash(&other.key),
        }
    }
}

pub fn validate_response(response: Response) -> std::result::Result<(), ()> {
    match response {
        Response::Values(values) => {
            if values.values().is_empty() {
                RESPONSE_MISS.increment();
                GET_KEY_MISS.increment();
            } else {
                RESPONSE_HIT.increment();
                GET_KEY_HIT.increment();
            }
            Ok(())
        }
        _ => {
            GET_EX.increment();
            Err(())
        }
    }
}
