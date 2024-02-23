//! Generate different proofs for the taiko protocol.

#[allow(dead_code)]
pub mod cache;
pub mod powdr;
pub mod pse_zk;
pub mod sgx;
pub mod succinct;

#[allow(dead_code)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
pub enum ProofType {
    Succinct,
    PseZk,
    Powdr,
    Sgx,
}
