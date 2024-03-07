//! Generate different proofs for the taiko protocol.
use crate::prover::{
    context::Context,
    request::{ProofRequest, SgxResponse},
};

#[allow(dead_code)]
pub mod cache;

// TODO: driver trait

#[cfg(feature = "powdr")]
pub mod powdr;
#[cfg(not(feature = "powdr"))]
pub mod powdr {

    pub async fn execute_powdr() -> Result<(), String> {
        Err("Feature not powdr is enabled".to_string())
    }
}

#[cfg(feature = "pse_zk")]
pub mod pse_zk;
#[cfg(not(feature = "pse_zk"))]
pub mod pse_zk {
    use super::*;
    pub async fn execute_pse(ctx: &mut Context, req: &ProofRequest) {
        println!("Feature not pse_zk is enabled");
    }
}

#[cfg(feature = "sgx")]
pub mod sgx;
#[cfg(not(feature = "sgx"))]
pub mod sgx {
    use super::*;
    pub async fn execute_sgx(ctx: &mut Context, req: &ProofRequest) -> Result<SgxResponse, String> {
        Err("Feature not sgx is enabled".to_string())
    }
}

#[cfg(feature = "succinct")]
pub mod succinct;
#[cfg(not(feature = "succinct"))]
pub mod succinct {
    use super::*;
    use crate::prover::request::SP1Response;
    pub async fn execute_sp1(ctx: &mut Context, req: &ProofRequest) -> Result<SP1Response, String> {
        Err("Feature not succinct is enabled".to_string())
    }
}

#[cfg(feature = "risc0")]
pub mod risc0;
#[cfg(not(feature = "risc0"))]
pub mod risc0 {
    use zeth_lib::{
        input::{GuestInput, GuestOutput},
        EthereumTxEssence,
    };

    use super::*;
    use crate::prover::request::Risc0ProofParams;
    pub async fn execute_risc0(
        input: GuestInput<EthereumTxEssence>,
        output: GuestOutput,
        ctx: &Context,
        req: &Risc0ProofParams,
    ) -> Result<Risc0Response, String> {
        Err("Feature not risc0 is enabled".to_string())
    }
}
