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

use alloy_primitives::{B256, U256};
use reth_revm::db::states::{PlainStorageChangeset, StateChangeset};
use reth_revm::db::BundleState;
use reth_revm::primitives::KECCAK_EMPTY;

pub trait Update {
    fn apply_changeset(&mut self, changeset: StateChangeset) -> anyhow::Result<()>;
    fn insert_block_hash(&mut self, block_number: U256, block_hash: B256) -> anyhow::Result<()>;
}

/// This function is a modified version of [`BundleState::into_plane_state`] from the revm crate:
/// https://github.com/bluealloy/revm/blob/4f093996c6059aad4db02b7eb03dca13e13be8a1/crates/revm/src/db/states/bundle_state.rs#L587
/// It retains account code for reuse instead of the default revm behavior to drop it.
pub fn into_plain_state(bundle: BundleState) -> StateChangeset {
    // pessimistically pre-allocate assuming _all_ accounts changed.
    let state_len = bundle.state.len();
    let mut accounts = Vec::with_capacity(state_len);
    let mut storage = Vec::with_capacity(state_len);

    for (address, account) in bundle.state {
        // append account info if it is changed.
        let was_destroyed = account.was_destroyed();
        if account.is_info_changed() {
            accounts.push((address, account.info));
        }

        // append storage changes

        // NOTE: Assumption is that revert is going to remove whole plain storage from
        // database so we can check if plain state was wiped or not.
        let mut account_storage_changed = Vec::with_capacity(account.storage.len());

        for (key, slot) in account.storage {
            // If storage was destroyed that means that storage was wiped.
            // In that case we need to check if present storage value is different then ZERO.
            let destroyed_and_not_zero = was_destroyed && !slot.present_value.is_zero();

            // If account is not destroyed check if original values was changed,
            // so we can update it.
            let not_destroyed_and_changed = !was_destroyed && slot.is_changed();

            if destroyed_and_not_zero || not_destroyed_and_changed {
                account_storage_changed.push((key, slot.present_value));
            }
        }

        if !account_storage_changed.is_empty() || was_destroyed {
            // append storage changes to account.
            storage.push(PlainStorageChangeset {
                address,
                wipe_storage: was_destroyed,
                storage: account_storage_changed,
            });
        }
    }
    let contracts = bundle
        .contracts
        .into_iter()
        // remove empty bytecodes
        .filter(|(b, _)| *b != KECCAK_EMPTY)
        .collect::<Vec<_>>();
    StateChangeset {
        accounts,
        storage,
        contracts,
    }
}
