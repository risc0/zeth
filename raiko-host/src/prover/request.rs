use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use zeth_lib::input::GuestOutput;
use zeth_primitives::{Address, B256};

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProofType {
    #[cfg(feature = "succinct")]
    Succinct,
    PseZk,
    Powdr,
    Sgx,
    Risc0(Risc0ProofParams),
    Native,
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Risc0ProofParams {
    pub bonsai: bool,
    pub snark: bool,
    pub profile: bool,
    pub execution_po2: u32,
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofRequest {
    /// the l2 block number
    pub block_number: u64,
    /// l2 node for get block by number
    pub l2_rpc: String,
    /// l1 node for signal root verify and get txlist info from proposed transaction.
    pub l1_rpc: String,
    /// beacon node for data blobs
    pub beacon_rpc: String,
    /// l2 contracts selection
    pub chain: String,
    // graffiti
    pub graffiti: B256,
    /// the protocol instance data
    #[serde_as(as = "DisplayFromStr")]
    pub prover: Address,

    pub proof_type: ProofType,
}

// Use Output type in Patar's Driver trait
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProofResponse {
    Sgx(SgxResponse),
    PseZk(PseZkResponse),
    #[cfg(feature = "succinct")]
    SP1(sp1_guest::SP1Response),
    Native(GuestOutput),
    Risc0(Risc0Response),
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

// #[derive(Clone, Serialize, Deserialize)]
// pub struct SP1Response {
//     pub proof: String,
//     pub output: GuestOutput,
// }

#[derive(Clone, Serialize, Deserialize)]
pub struct Risc0Response {
    pub journal: String,
}
