use super::*;

/// Sets a key-value pair in the cache.
pub async fn set(
    connection: &mut MultiplexedConnection,
    config: &Config,
    request: workload::client::Set,
) -> std::result::Result<(), ResponseError> {
    SET.increment();

    let key = &*request.key;
    let value = &*request.value;

    let mut base_command = ::redis::cmd("SET");

    let mut command = base_command.arg(key).arg(value);

    if let Some(ttl) = request.ttl {
        if ttl.subsec_nanos() == 0 {
            command = command.arg("EX").arg(ttl.as_secs());
        } else {
            command = command.arg("PX").arg(ttl.as_millis() as u64);
        }
    }

    match timeout(
        config.client().unwrap().request_timeout(),
        command.query_async(connection),
    )
    .await
    {
        Ok(Ok(())) => {
            SET_STORED.increment();
            Ok(())
        }
        Ok(Err(_)) => {
            SET_EX.increment();
            Err(ResponseError::Exception)
        }
        Err(_) => {
            SET_TIMEOUT.increment();
            Err(ResponseError::Timeout)
        }
    }
}
