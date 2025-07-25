// Copyright 2025 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use actix_web::{App, HttpResponse, HttpServer, Responder, web};
use alloy::{
    eips::BlockNumberOrTag,
    providers::{DynProvider, Provider, ProviderBuilder},
    rpc::client::RpcClient,
    transports::layers::RetryBackoffLayer,
};
use alloy_chains::NamedChain;
use anyhow::{Context, bail};
use clap::Parser;
use reqwest::Client;
use reth_chainspec::{HOLESKY, HOODI, MAINNET, SEPOLIA};
use reth_evm_ethereum::EthEvmConfig;
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::{debug, error, field, info, instrument};
use tracing_actix_web::TracingLogger;
use zeth_rpc_proxy::execution_witness;

/// This struct holds the application state that we want to share across all handlers.
struct AppState {
    client: Client,
    upstream_url: String,
    provider: DynProvider,
    evm_config: Arc<EthEvmConfig>,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The upstream RPC provider URL to forward requests to.
    #[arg(long, env)]
    rpc_url: String,

    /// The network address and port to bind the server to.
    #[arg(long, default_value = "127.0.0.1:8545")]
    bind_address: String,

    /// The initial backoff in milliseconds.
    #[clap(long, default_value_t = 500)]
    pub rpc_retry_backoff: u64,

    /// The number of allowed Compute Units per second.
    #[clap(long, default_value_t = 1000)]
    pub rpc_retry_cu: u64,
}

/// This function is the entry point for all incoming RPC requests.
/// It checks the method and either forwards it or handles it locally.
#[instrument(skip_all, fields(method = field::Empty))]
async fn rpc_handler(body: web::Bytes, data: web::Data<AppState>) -> impl Responder {
    // Deserialize the incoming request into a generic JSON Value.
    let request: Value = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(e) => {
            // If JSON is malformed, return a proper JSON-RPC error.
            error!(error = %e, "Failed to parse JSON request");
            return HttpResponse::BadRequest().json(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32700, "message": format!("Parse error: {}", e) },
                "id": null
            }));
        }
    };

    // Extract method, id, and params from the request.
    let method = request.get("method").and_then(Value::as_str);
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    tracing::Span::current().record("method", method.unwrap_or("unknown"));

    match method {
        // If the method is `debug_executionWitness`, handle it locally.
        Some("debug_executionWitness") => handle_debug_execution_witness(id, params, data).await,
        // For all other methods, forward the request to the upstream provider.
        Some(_) => forward_request(&body, &data.client, &data.upstream_url).await,
        // If the method is not specified, return an error.
        None => {
            error!("Request is missing 'method' field.");
            HttpResponse::BadRequest().json(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32600, "message": "Invalid Request: method not found" },
                "id": id
            }))
        }
    }
}

/// Forwards the raw request body to the upstream provider and returns the response.
async fn forward_request(body: &web::Bytes, client: &Client, upstream_url: &str) -> HttpResponse {
    debug!("Forwarding request to upstream");
    match client
        .post(upstream_url)
        .header("Content-Type", "application/json")
        .body(body.clone())
        .send()
        .await
    {
        Ok(upstream_response) => {
            // convert reqwest::StatusCode to actix_web::http::StatusCode.
            let status_code =
                actix_web::http::StatusCode::from_u16(upstream_response.status().as_u16())
                    .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR);

            let mut response_builder = HttpResponse::build(status_code);

            // Copy headers from the upstream response to the new response.
            for (name, value) in upstream_response.headers().iter() {
                response_builder.insert_header((name.as_str(), value.as_bytes()));
            }

            // Get the raw bytes of the upstream response body.
            let body_bytes = match upstream_response.bytes().await {
                Ok(bytes) => bytes,
                Err(e) => {
                    error!(error = %e, "Failed to read upstream response body");
                    return HttpResponse::InternalServerError().json(json!({
                        "jsonrpc": "2.0",
                        "error": { "code": -32000, "message": format!("Upstream response body error: {}", e) },
                        "id": null // ID might not be available here
                    }));
                }
            };

            response_builder.body(body_bytes)
        }
        Err(e) => {
            // Handle errors in reaching the upstream provider.
            error!(error = %e, "Error forwarding request to upstream provider");
            HttpResponse::InternalServerError().json(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32001, "message": format!("Upstream provider error: {}", e) },
                "id": null
            }))
        }
    }
}

/// This is the custom implementation for the `debug_executionWitness` RPC call.
async fn handle_debug_execution_witness(
    id: Value,
    params: Value,
    data: web::Data<AppState>,
) -> HttpResponse {
    debug!("Handling 'debug_executionWitness' locally");

    let params_vec: Vec<BlockNumberOrTag> = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(_) => {
            return HttpResponse::BadRequest().json(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32602, "message": "Invalid params" },
                "id": id
            }));
        }
    };
    if params_vec.len() != 1 {
        return HttpResponse::BadRequest().json(json!({
            "jsonrpc": "2.0",
            "error": { "code": -32602, "message": "Invalid params: expected a single BlockNumberOrTag parameter" },
            "id": id
        }));
    }

    let block_id = params_vec[0];
    match execution_witness(data.evm_config.clone(), &data.provider, block_id).await {
        Ok(witness) => HttpResponse::Ok().json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": witness
        })),
        Err(e) => {
            error!(error = format!("{e:#}"), "Preflight function failed");
            HttpResponse::InternalServerError().json(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32000, "message": format!("Preflight error: {}", e) },
                "id": id
            }))
        }
    }
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let retry = RetryBackoffLayer::new(10, args.rpc_retry_backoff, args.rpc_retry_cu);
    let client = RpcClient::builder().layer(retry).connect(&args.rpc_url).await?;

    let provider = ProviderBuilder::new().connect_client(client);
    let chain_id = provider.get_chain_id().await.context("eth_chainId failed")?;
    let chain: NamedChain = chain_id.try_into().context("Invalid chain_id")?;
    let evm_config = match chain {
        NamedChain::Mainnet => Arc::new(EthEvmConfig::ethereum(MAINNET.clone())),
        NamedChain::Holesky => Arc::new(EthEvmConfig::ethereum(HOLESKY.clone())),
        NamedChain::Hoodi => Arc::new(EthEvmConfig::ethereum(HOODI.clone())),
        NamedChain::Sepolia => Arc::new(EthEvmConfig::ethereum(SEPOLIA.clone())),
        _ => bail!("Unsupported chain: {chain}"),
    };
    info!("EVM config: {}", chain);

    // Create the shared application state.
    // web::Data handles the atomic reference counting for safe sharing across threads.
    let app_state = web::Data::new(AppState {
        client: Client::new(),
        upstream_url: args.rpc_url,
        provider: provider.erased(),
        evm_config,
    });

    info!(
        bind_address = %args.bind_address,
        upstream_url = %app_state.upstream_url,
        "Starting RPC proxy server"
    );

    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .wrap(TracingLogger::default())
            .route("/", web::post().to(rpc_handler))
    })
    .bind(args.bind_address)?
    .run()
    .await?;

    Ok(())
}
