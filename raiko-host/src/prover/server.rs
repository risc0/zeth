use std::path::{Path, PathBuf};

use hyper::{
    body::{Buf, HttpBody},
    header::HeaderValue,
    service::{make_service_fn, service_fn},
    Body, Method, Request, Response, Server, StatusCode,
};
use once_cell::sync::{Lazy, OnceCell};
use prometheus::{Encoder, TextEncoder};
use tower::ServiceBuilder;
use tracing::info;

use crate::prover::{
    context::Context,
    execution::execute,
    json_rpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, JsonRpcResponseError},
    proof::cache::Cache,
    request::*,
};

pub static SGX_INSTANCE_ID: OnceCell<u32> = OnceCell::new();

/// Starts the proverd json-rpc server.
/// Note: the server may not immediately listening after returning the
/// `JoinHandle`.
#[allow(clippy::too_many_arguments)]
pub fn serve(
    addr: &str,
    guest_elf: &Path,
    host_cache: &Path,
    l2_contracts: &str,
    sgx_instance_id: u32,
    proof_cache: usize,
    concurrency_limit: usize,
    max_caches: usize,
) -> tokio::task::JoinHandle<()> {
    let addr = addr
        .parse::<std::net::SocketAddr>()
        .expect("valid socket address");
    let guest_elf = guest_elf.to_owned();
    let host_cache = host_cache.to_owned();
    let l2_contracts = l2_contracts.to_owned();
    SGX_INSTANCE_ID.set(sgx_instance_id);
    tokio::spawn(async move {
        let handler = Handler::new(
            guest_elf.clone(),
            host_cache.clone(),
            l2_contracts.clone(),
            // sgx_instance_id,
            proof_cache,
            max_caches,
        );
        let service = service_fn(move |req| {
            let handler = handler.clone();
            handler.handle_request(req)
        });

        let service = ServiceBuilder::new()
            .concurrency_limit(concurrency_limit)
            .service(service);

        let service = make_service_fn(|_| {
            let service = service.clone();
            async move { Ok::<_, hyper::Error>(service) }
        });

        let server = Server::bind(&addr).serve(service);
        info!("Listening on http://{}", addr);
        server.await.expect("server should be serving");
    })
}

/// sets default headers for CORS requests
fn set_headers(headers: &mut hyper::HeaderMap, extended: bool) {
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));

    if extended {
        headers.insert(
            "access-control-allow-methods",
            HeaderValue::from_static("post, get, options"),
        );
        headers.insert(
            "access-control-allow-headers",
            HeaderValue::from_static("origin, content-type, accept, x-requested-with"),
        );
        headers.insert("access-control-max-age", HeaderValue::from_static("300"));
    }
}

#[derive(Clone)]
struct Handler {
    ctx: Context,
    cache: Cache,
}

impl Handler {
    fn new(
        guest_elf: PathBuf,
        host_cache: PathBuf,
        l2_contracts: String,
        capacity: usize,
        max_caches: usize,
    ) -> Self {
        Self {
            ctx: Context::new(guest_elf, host_cache, max_caches, None),
            cache: Cache::new(capacity),
        }
    }

    async fn handle_request(mut self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        {
            // limits the request size
            const MAX_BODY_SIZE: u64 = 1 << 20;
            let response_content_length = match req.body().size_hint().upper() {
                Some(v) => v,
                None => MAX_BODY_SIZE + 1,
            };

            if response_content_length > MAX_BODY_SIZE {
                let mut resp = Response::new(Body::from("request too large"));
                *resp.status_mut() = StatusCode::BAD_REQUEST;
                return Ok(resp);
            }
        }

        match (req.method(), req.uri().path()) {
            (&Method::GET, "/health") => {
                // nothing to report yet - healthy by default
                let mut resp = Response::default();
                set_headers(resp.headers_mut(), false);
                Ok(resp)
            }

            // json-rpc
            (&Method::POST, "/") => {
                let body_bytes = hyper::body::aggregate(req.into_body())
                    .await
                    .unwrap()
                    .reader();
                let json_req: Result<JsonRpcRequest<Vec<serde_json::Value>>, serde_json::Error> =
                    serde_json::from_reader(body_bytes);

                if let Err(err) = json_req {
                    let payload = serde_json::to_vec(&JsonRpcResponseError {
                        jsonrpc: "2.0".to_string(),
                        id: 0.into(),
                        error: JsonRpcError {
                            // parser error
                            code: -32700,
                            message: err.to_string(),
                        },
                    })
                    .unwrap();
                    let mut resp = Response::new(Body::from(payload));
                    set_headers(resp.headers_mut(), false);
                    return Ok(resp);
                }

                let json_req = json_req.unwrap();
                let result: Result<serde_json::Value, String> = self
                    .handle_method(json_req.method.as_str(), &json_req.params)
                    .await;
                let payload = match result {
                    Err(err) => {
                        serde_json::to_vec(&JsonRpcResponseError {
                            jsonrpc: "2.0".to_string(),
                            id: json_req.id,
                            error: JsonRpcError {
                                // internal server error
                                code: -32000,
                                message: err,
                            },
                        })
                    }
                    Ok(val) => serde_json::to_vec(&JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: json_req.id,
                        result: Some(val),
                    }),
                };
                let mut resp = Response::new(Body::from(payload.unwrap()));
                set_headers(resp.headers_mut(), false);
                Ok(resp)
            }

            // serve CORS headers
            (&Method::OPTIONS, "/") => {
                let mut resp = Response::default();
                set_headers(resp.headers_mut(), true);
                Ok(resp)
            }

            // serve metrics
            (&Method::GET, "/metrics") => {
                let encoder = TextEncoder::new();
                let mut buffer = vec![];
                let mf = prometheus::gather();
                encoder.encode(&mf, &mut buffer).unwrap();
                let resp = Response::builder()
                    .header(hyper::header::CONTENT_TYPE, encoder.format_type())
                    .body(Body::from(buffer))
                    .unwrap();
                Ok(resp)
            }

            // everything else
            _ => {
                let mut not_found = Response::default();
                *not_found.status_mut() = StatusCode::NOT_FOUND;
                Ok(not_found)
            }
        }
    }

    async fn handle_method(
        &mut self,
        method: &str,
        params: &[serde_json::Value],
    ) -> Result<serde_json::Value, String> {
        match method {
            // enqueues a task for computating proof for any given block
            "proof" => {
                println!("prodfasdfkasj;dkflja;sldk");
                let options = params.first().ok_or("expected struct ProofRequest")?;
                let req: ProofRequest =
                    serde_json::from_value(options.to_owned()).map_err(|e| e.to_string())?;
                execute(&self.cache, &mut self.ctx, &req)
                    .await
                    .and_then(|result| serde_json::to_value(result).map_err(Into::into))
                    .map_err(|e| e.to_string())
                // Ok(serde_json::Value::Bool(false))
            }
            _ => todo!(),
        }
    }
}
