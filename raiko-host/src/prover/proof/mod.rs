//! Generate different proofs for the taiko protocol.

#[allow(dead_code)]
pub mod cache;

#[cfg(feature = "powdr")]
pub mod powdr;
pub mod pse_zk;
pub mod sgx;
#[cfg(feature = "succinct")]
pub mod succinct;
pub mod risc0;

#[allow(dead_code)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
pub enum ProofType {
    #[cfg(feature = "succinct")]
    Succinct,
    PseZk,
    #[cfg(feature = "powdr")]
    Powdr,
    Sgx,
}
