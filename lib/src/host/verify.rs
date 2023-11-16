// Copyright 2023 RISC Zero, Inc.
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

use anyhow::{bail, Context, Result};
use ethers_core::types::EIP1186ProofResponse;
use hashbrown::HashMap;
use log::error;
use zeth_primitives::{
    block::Header,
    keccak::keccak,
    transactions::TxEssence,
    trie::{Error as TrieError, MptNode, StateAccount},
    Address, B256, U256,
};

use super::preflight;

#[derive(Debug)]
pub enum VerifyError {
    BalanceMismatch {
        rpc_value: U256,
        our_value: U256,
        difference: U256,
    },
    NonceMismatch {
        rpc_value: u64,
        our_value: u64,
    },
    CodeHashMismatch {
        rpc_value: B256,
        our_value: B256,
    },
    StorageMismatch {
        index: U256,
        rpc_value: U256,
        our_db_value: U256,
        our_trie_value: U256,
    },
    StorageRootMismatch {
        rpc_value: B256,
        our_value: B256,
    },
    DeletedAccountMismatch,
    UnresolvedAccount,
}

/// Verify the block header and state trie.
pub trait Verifier {
    fn verify_block(&self, header: &Header, state: &MptNode) -> Result<()>;
}

/// Verify using the preflight data.
impl<E: TxEssence> Verifier for preflight::Data<E> {
    fn verify_block(&self, header: &Header, state: &MptNode) -> Result<()> {
        let errors =
            verify_state_trie(state, &self.proofs).context("failed to verify state trie")?;

        for (address, address_errors) in &errors {
            error!(
                "Verify found {:?} error(s) for address {:?}",
                address_errors.len(),
                address
            );
            for error in address_errors {
                error!("  Error: {:?}", error);
            }
        }

        let accounts_len = self.proofs.len();
        let errors_len = errors.len();
        if errors_len > 0 {
            error!(
                "Verify found {:?} account(s) with error(s) ({}% correct)",
                errors_len,
                (100.0 * (accounts_len - errors_len) as f64 / accounts_len as f64)
            );
        }

        verify_header(header, &self.header)
    }
}

fn verify_header(header: &Header, exp_header: &Header) -> Result<()> {
    if header.state_root != exp_header.state_root {
        error!(
            "State root mismatch {} (expected {})",
            header.state_root, exp_header.state_root
        );
    }

    if header.transactions_root != exp_header.transactions_root {
        error!(
            "Transactions root mismatch {} (expected {})",
            header.transactions_root, exp_header.transactions_root
        );
    }

    if header.receipts_root != exp_header.receipts_root {
        error!(
            "Receipts root mismatch {} (expected {})",
            header.receipts_root, exp_header.receipts_root
        );
    }

    if header.base_fee_per_gas != exp_header.base_fee_per_gas {
        error!(
            "Base fee mismatch {} (expected {})",
            header.base_fee_per_gas, exp_header.base_fee_per_gas
        );
    }

    if header.withdrawals_root != exp_header.withdrawals_root {
        error!(
            "Withdrawals root mismatch {:?} (expected {:?})",
            header.withdrawals_root, exp_header.withdrawals_root
        );
    }

    let found_hash = header.hash();
    let expected_hash = exp_header.hash();
    if found_hash.as_slice() != expected_hash.as_slice() {
        error!(
            "Final block hash mismatch {} (expected {})",
            found_hash, expected_hash,
        );

        bail!("Invalid block hash");
    }

    Ok(())
}

fn verify_state_trie(
    state_trie: &MptNode,
    proofs: &HashMap<Address, EIP1186ProofResponse>,
) -> Result<HashMap<Address, Vec<VerifyError>>> {
    let mut errors = HashMap::new();

    for (address, proof_response) in proofs {
        let rpc_account: StateAccount = proof_response.clone().into();

        let mut address_errors = Vec::new();
        match state_trie.get_rlp::<StateAccount>(&keccak(address)) {
            Ok(account) => match account {
                // the account is not in the trie
                None => {
                    // the account was deleted, so the RPC account should be empty
                    if rpc_account != Default::default() {
                        address_errors.push(VerifyError::DeletedAccountMismatch);
                    }
                }
                Some(account_info) => {
                    // Account balance
                    {
                        let rpc_value = rpc_account.balance;
                        let our_value = account_info.balance;
                        if rpc_value != our_value {
                            let difference = rpc_value.abs_diff(our_value);
                            address_errors.push(VerifyError::BalanceMismatch {
                                rpc_value,
                                our_value,
                                difference,
                            })
                        }
                    }

                    // Nonce
                    {
                        let rpc_value = rpc_account.nonce;
                        let our_value = account_info.nonce;
                        if rpc_value != our_value {
                            address_errors.push(VerifyError::NonceMismatch {
                                rpc_value,
                                our_value,
                            })
                        }
                    }

                    // Code hash
                    {
                        let rpc_value = rpc_account.code_hash;
                        let our_value = account_info.code_hash;
                        if rpc_value != our_value {
                            address_errors.push(VerifyError::CodeHashMismatch {
                                rpc_value,
                                our_value,
                            })
                        }
                    }

                    // Storage root
                    {
                        let rpc_value = rpc_account.storage_root;
                        let our_value = account_info.storage_root;
                        if rpc_value != our_value {
                            address_errors.push(VerifyError::StorageRootMismatch {
                                rpc_value,
                                our_value,
                            });
                        }
                    }
                }
            },
            // the account was pruned from the trie
            Err(TrieError::NodeNotResolved(_)) => {
                address_errors.push(VerifyError::UnresolvedAccount);
            }
            Err(err) => {
                bail!("Error while fetching account {:?}: {:?}", address, err);
            }
        }

        if !address_errors.is_empty() {
            errors.insert(address.to_owned(), address_errors);
        }
    }

    Ok(errors)
}
