/// Common utilities for json-rpc
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponseError {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub error: JsonRpcError,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse<T> {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub result: Option<T>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest<T: Serialize> {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    pub params: T,
}

// curl --location --request POST 'http://localhost:8080' --header 'Content-Type: application/json' --data-raw '{-raw '{
// "jsonrpc": "2.0",
// "id": 1,
// "method": "proof",
// "params": [
// {
// "type": "Sgx",
// "l2Rpc": "https://rpc.internal.taiko.xyz",
// "l1Rpc": "https://l1rpc.internal.taiko.xyz",
// "l2Contracts": "testnet",
// "proofInstance": "powdr",
// "block": 2,
// "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
// "graffiti": "0000000000000000000000000000000000000000000000000000000000000000"
// }
// ]
// }'
