// Copyright 2024 RISC Zero, Inc.
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

use std::sync::Arc;
use alloy_primitives::U256;
use alloy_sol_types::{sol, SolValue};
use anyhow::anyhow;
use bonsai_sdk::alpha::responses::{Groth16Seal, SnarkReceipt};
use ethers_contract::abigen;
use ethers_core::types::H160;
use ethers_providers::{Http, Provider, RetryClient};
use risc0_zkvm::sha::{Digest, Digestible};

static RISC_ZERO_VERIFIER: ethers_core::types::Address = H160::zero();

sol!(
    /// A Groth16 seal over the claimed receipt claim.
    struct Seal {
        uint256[2] a;
        uint256[2][2] b;
        uint256[2] c;
    }
    /// Verifier interface for RISC Zero receipts of execution.
    #[derive(Debug)]
    interface RiscZeroVerifier {
        /// Verify that the given seal is a valid RISC Zero proof of execution with the
        /// given image ID, post-state digest, and journal digest. This method additionally
        /// ensures that the input hash is all-zeros (i.e. no committed input), the exit code
        /// is (Halted, 0), and there are no assumptions (i.e. the receipt is unconditional).
        /// Returns true if the receipt passes the verification checks. The return code must be checked.
        function verify(
            /// The encoded cryptographic proof (i.e. SNARK).
            bytes calldata seal,
            /// The identifier for the guest program.
            bytes32 imageId,
            /// A hash of the final memory state. Required to run the verifier, but otherwise can be left unconstrained for most use cases.
            bytes32 postStateDigest,
            /// The SHA-256 digest of the journal bytes.
            bytes32 journalDigest
        )
            external
            view
        returns (bool);
    }
);

abigen!(
    IRiscZeroVerifier,
    r#"[
        function verify(bytes calldata seal, bytes32 imageId, bytes32 postStateDigest, bytes32 journalDigest) external view returns (bool)
    ]"#
);

fn to_u256_arr<const N: usize>(be_vecs: &[Vec<u8>]) -> [U256; N] {
    let tmp: Vec<_> = be_vecs
        .iter()
        .map(|v| U256::from_be_slice(v.as_slice()))
        .collect();
    tmp.try_into().unwrap()
}

impl From<Groth16Seal> for Seal {
    fn from(val: Groth16Seal) -> Self {
        Seal {
            a: to_u256_arr(&val.a),
            b: [to_u256_arr(&val.b[0]), to_u256_arr(&val.b[1])],
            c: to_u256_arr(&val.c),
        }
    }
}

pub async fn verify_groth16_snark(
    image_id: Digest,
    snark_receipt: SnarkReceipt,
) -> anyhow::Result<()> {
    let verifier_rpc_url = "TODO: http://fuckBonsai:8545";

    let http_client = Arc::new(Provider::<RetryClient<Http>>::new_client(
        &verifier_rpc_url,
        3,
        500,
    )?);

    let seal = <Groth16Seal as Into<Seal>>::into(snark_receipt.snark).abi_encode();
    let journal_digest = snark_receipt.journal.digest();
    log::info!("Verifying SNARK:");
    log::info!("Seal: {}", hex::encode(&seal));
    log::info!("Image ID: {}", hex::encode(image_id.as_bytes()));
    log::info!(
        "Post State Digest: {}",
        hex::encode(&snark_receipt.post_state_digest)
    );
    log::info!("Journal Digest: {}", hex::encode(journal_digest.as_bytes()));
    let verification = IRiscZeroVerifier::new(RISC_ZERO_VERIFIER, http_client)
        .verify(
            seal.into(),
            image_id.as_bytes().try_into().unwrap(),
            snark_receipt
                .post_state_digest
                .as_slice()
                .try_into()
                .unwrap(),
            journal_digest.as_bytes().try_into().unwrap(),
        )
        .await?;

    if verification {
        log::info!("SNARK verified successfully using {}!", RISC_ZERO_VERIFIER);
    } else {
        log::error!("SNARK verification failed!");
    }

    Ok(())
}
