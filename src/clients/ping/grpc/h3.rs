use crate::clients::common::Queue;
use crate::workload::ClientRequest;
use crate::workload::ClientWorkItemKind;
use crate::*;
use async_channel::Receiver;
use bytes::Bytes;
use chrono::Utc;
use h3::client::SendRequest;
use http::uri::Authority;
use http::HeaderValue;
use http::Method;
use http::Version;
use rustls::pki_types::CertificateDer;
use std::io::Error;
use std::io::ErrorKind;
use std::time::Instant;
use tokio::runtime::Runtime;

static ALPN: &[u8] = b"h3";

// launch a pool manager and worker tasks since HTTP/3 is mux'ed we prepare
// senders in the pool manager and pass them over a queue to our worker tasks
pub fn launch_tasks(
    runtime: &mut Runtime,
    config: Config,
    work_receiver: Receiver<ClientWorkItemKind<ClientRequest>>,
) {
    debug!("launching ping http3 protocol tasks");

    for _ in 0..config.client().unwrap().poolsize() {
        for endpoint in config.target().endpoints() {
            // for each endpoint have poolsize # of pool_managers, each managing
            // a single H3 stream

            let queue = Queue::new(1);
            runtime.spawn(pool_manager(
                endpoint.clone(),
                config.clone(),
                queue.clone(),
            ));

            // since HTTP/3.0 allows muxing several sessions onto a single H3
            // stream, we launch one task for each session on this H3 stream
            for _ in 0..config.client().unwrap().concurrency() {
                runtime.spawn(task(
                    work_receiver.clone(),
                    endpoint.clone(),
                    config.clone(),
                    queue.clone(),
                ));
            }
        }
    }
}

async fn resolve(uri: &str) -> Result<(std::net::SocketAddr, Authority), std::io::Error> {
    let uri = uri
        .parse::<http::Uri>()
        .map_err(|_| Error::new(ErrorKind::Other, "failed to parse uri"))?;

    let auth = uri
        .authority()
        .ok_or(Error::new(ErrorKind::Other, "uri has no authority"))?
        .clone();

    let port = auth.port_u16().unwrap_or(443);

    let addr = tokio::net::lookup_host((auth.host(), port))
        .await?
        .next()
        .ok_or(Error::new(ErrorKind::Other, "dns found no addresses"))?;

    Ok((addr, auth))
}

fn root_cert_store(config: &Config) -> rustls::RootCertStore {
    // load system CA certs
    let mut roots = rustls::RootCertStore::empty();

    let r = rustls_native_certs::load_native_certs();

    if r.certs.is_empty() {
        eprintln!("couldn't load any default trust roots:");
        for error in r.errors {
            eprintln!("{error}");
        }
    }

    for cert in r.certs {
        if let Err(e) = roots.add(cert) {
            eprintln!("failed to parse trust anchor: {}", e);
        }
    }

    if let Some(Some(ca_file)) = config.tls().map(|c| c.ca_file()) {
        if let Err(e) = roots.add(CertificateDer::from(std::fs::read(ca_file).unwrap())) {
            eprintln!("failed to parse trust anchor: {}", e);
        } else {
            eprintln!("added cert from file: {ca_file}");
        }
    }

    roots
}

pub async fn pool_manager(
    endpoint: String,
    config: Config,
    queue: Queue<SendRequest<h3_quinn::OpenStreams, Bytes>>,
) {
    let mut client = None;

    let mut tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_cert_store(&config))
        .with_no_client_auth();

    tls_config.enable_early_data = true;
    tls_config.alpn_protocols = vec![ALPN.into()];

    let quic_client_config = Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(tls_config)
            .expect("failed to initialize quic client config"),
    );

    while RUNNING.load(Ordering::Relaxed) {
        if client.is_none() {
            CONNECT.increment();

            if let Ok((addr, auth)) = resolve(&endpoint).await {
                if let Ok(mut client_endpoint) =
                    h3_quinn::quinn::Endpoint::client("[::]:0".parse().unwrap())
                {
                    let mut client_config = quinn::ClientConfig::new(quic_client_config.clone());

                    let mut transport_config = quinn::TransportConfig::default();
                    transport_config.keep_alive_interval(Some(Duration::from_secs(1)));
                    client_config.transport_config(Arc::new(transport_config));

                    client_endpoint.set_default_client_config(client_config);

                    if let Ok(quic_conn) = client_endpoint
                        .connect(addr, auth.host())
                        .unwrap()
                        .await
                        .map_err(|e| {
                            eprintln!("failed to create http3 client: {e}");
                        })
                    {
                        let quinn_conn = h3_quinn::Connection::new(quic_conn);

                        if let Ok((mut driver, send_request)) = ::h3::client::new(quinn_conn).await
                        {
                            CONNECT_OK.increment();
                            CONNECT_CURR.increment();

                            tokio::spawn(async move {
                                let _ = core::future::poll_fn(|cx| driver.poll_close(cx)).await;
                            });

                            client = Some(send_request);

                            continue;
                        }
                    }
                }
            }

            // Successfully negotiated connections result in early continue back
            // to the top of the loop. If we hit this, that means there was some
            // exception during connection establishment / negotiation.
            CONNECT_EX.increment();
        } else if let Some(s) = client.clone() {
            let _ = queue.send(s).await;
        }
    }
}

// a task for HTTP/3.0
#[allow(clippy::slow_vector_initialization)]
async fn task(
    work_receiver: Receiver<ClientWorkItemKind<ClientRequest>>,
    endpoint: String,
    _config: Config,
    queue: Queue<SendRequest<h3_quinn::OpenStreams, Bytes>>,
) -> Result<(), std::io::Error> {
    let uri = endpoint
        .parse::<http::Uri>()
        .map_err(|_| Error::new(ErrorKind::Other, "failed to parse uri"))?;

    let auth = uri
        .authority()
        .ok_or(Error::new(ErrorKind::Other, "uri has no authority"))?
        .clone();

    let _port = auth.port_u16().unwrap_or(443);

    while RUNNING.load(Ordering::Relaxed) {
        let sender = queue.recv().await;

        if sender.is_err() {
            continue;
        }

        let mut sender = sender.unwrap();

        let work_item = work_receiver
            .recv()
            .await
            .map_err(|_| Error::new(ErrorKind::Other, "channel closed"))?;

        REQUEST.increment();

        // compose request into buffer
        match &work_item {
            ClientWorkItemKind::Request { request, .. } => match request {
                ClientRequest::Ping(_) => {}
                _ => {
                    REQUEST_UNSUPPORTED.increment();
                    continue;
                }
            },
            ClientWorkItemKind::Reconnect => {
                REQUEST_UNSUPPORTED.increment();
                continue;
            }
        };

        let mut date = HeaderValue::from_str(&Utc::now().to_rfc2822()).unwrap();
        date.set_sensitive(true);

        let request = http::request::Builder::new()
            .version(Version::HTTP_3)
            .method(Method::POST)
            .uri(&format!("https://{auth}/pingpong.Ping/Ping"))
            .header("content-type", "application/grpc")
            .header("date", date)
            .header("user-agent", "unknown/0.0.0")
            .header("te", "trailers")
            .body(())
            .unwrap();

        let start = Instant::now();

        if let Ok(mut stream) = sender.send_request(request).await {
            if stream
                .send_data(Bytes::from(vec![0, 0, 0, 0, 0]))
                .await
                .is_ok()
            {
                if stream.finish().await.is_err() {
                    continue;
                } else {
                    REQUEST_OK.increment();
                }

                if let Ok(_response) = stream.recv_response().await {
                    let stop = Instant::now();

                    RESPONSE_OK.increment();
                    PING_OK.increment();

                    let latency = stop.duration_since(start).as_nanos() as u64;
                    let _ = RESPONSE_LATENCY.increment(latency);
                } else {
                    RESPONSE_EX.increment();
                }
            }
        }
    }

    Ok(())
}
