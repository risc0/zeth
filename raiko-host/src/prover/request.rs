use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use zeth_lib::input::GuestOutput;
use zeth_primitives::{Address, B256};

#[serde_as]
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProofInstance {
    Succinct,
    PseZk,
    Powdr,
    Sgx,
    Risc0(Risc0Instance),
    Native,
}

#[serde_as]
#[derive(Clone, Serialize, Deserialize)]
pub struct Risc0Instance {
    pub bonsai: bool,
    pub snark: bool,
    pub profile: bool,
    pub execution_po2: u32,
}

#[serde_as]
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofRequest {
    /// the l2 block number
    pub block_number: u64,
    /// l2 node for get block by number
    pub l2_rpc: String,
    /// l1 node for signal root verify and get txlist info from proposed transaction.
    pub l1_rpc: String,
    /// l2 contracts selection
    pub l2_contracts: String,
    // graffiti
    pub graffiti: B256,
    /// the protocol instance data
    #[serde_as(as = "DisplayFromStr")]
    pub prover: Address,

    pub proof_instance: ProofInstance,
}

// curl --location --request POST 'http://localhost:8080' --header 'Content-Type: application/json' --data-raw '{
// "jsonrpc": "2.0",
// "id": 1,
// "method": "proof",
// "params": [
// {
// "type": "Sgx",
// "l2Rpc": "https://rpc.internal.taiko.xyz",
// "l1Rpc": "https://l1rpc.internal.taiko.xyz",
// "l2Contracts": "internal_devnet_a",
// "proofInstance": "native",
// "block": 2,
// "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
// "graffiti": "0000000000000000000000000000000000000000000000000000000000000000"
// }
// ]
// }'
//

// Use Output type in Patar's Driver trait
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProofResponse {
    Sgx(SgxResponse),
    PseZk(PseZkResponse),
    SP1(SP1Response),
    Native(GuestOutput),
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SgxResponse {
    /// proof format: 4b(id)+20b(pubkey)+65b(signature)
    pub proof: String,
    pub quote: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PseZkResponse {}

#[derive(Clone, Serialize, Deserialize)]
pub struct SP1Response {
    pub proof: String,
    pub pi_hash: String,
}
