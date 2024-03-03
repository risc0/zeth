use std::path::PathBuf;

use alloy_primitives::Address;
use alloy_sol_types::SolCall;
use anyhow::{anyhow, bail, Context, Result};
use ethers_core::types::{Block, Transaction};

use crate::{
    consts::ChainSpec,
    host::provider::{new_provider, BlockQuery, Provider, TxQuery},
    input::{anchorCall, decode_anchor, proposeBlockCall, BlockProposed}, taiko_utils::get_contracts,
};

pub struct TaikoProvider {
    pub l1_provider: Box<dyn Provider>,
    pub l2_provider: Box<dyn Provider>,
    pub l2_spec: Option<ChainSpec>,
}

impl TaikoProvider {
    pub fn new(
        l1_cache: Option<PathBuf>,
        l1_rpc: Option<String>,
        l2_cache: Option<PathBuf>,
        l2_rpc: Option<String>,
    ) -> Result<Self> {
        Ok(Self {
            l1_provider: new_provider(l1_cache, l1_rpc)?,
            l2_provider: new_provider(l2_cache, l2_rpc)?,
            l2_spec: None,
        })
    }

    pub fn with_l2_spec(mut self, spec: ChainSpec) -> Self {
        self.l2_spec = Some(spec);
        self
    }

    pub fn save(&mut self) -> Result<()> {
        self.l1_provider.save()?;
        self.l2_provider.save()?;
        Ok(())
    }

    pub fn get_l1_full_block(&mut self, l1_block_no: u64) -> Result<Block<Transaction>> {
        self.l1_provider.get_full_block(&BlockQuery {
            block_no: l1_block_no,
        })
    }

    pub fn get_l2_full_block(&mut self, l2_block_no: u64) -> Result<Block<Transaction>> {
        self.l2_provider.get_full_block(&BlockQuery {
            block_no: l2_block_no,
        })
    }

    pub fn get_anchor(&self, l2_block: &Block<Transaction>) -> Result<(Transaction, anchorCall)> {
        let tx = l2_block.transactions[0].clone();
        let call = decode_anchor(tx.input.as_ref())?;
        Ok((tx, call))
    }

    pub fn get_proposal(
        &mut self,
        l1_block_no: u64,
        l2_block_no: u64,
        chain_name: &str,
    ) -> Result<(proposeBlockCall, BlockProposed)> {
        let logs = self.l1_provider.filter_event_log::<BlockProposed>(
            get_contracts(chain_name).unwrap().0,
            l1_block_no,
            l2_block_no,
        )?;
        for (log, event) in logs {
            if event.blockId == zeth_primitives::U256::from(l2_block_no) {
                let tx = self
                    .l1_provider
                    .get_transaction(&TxQuery {
                        tx_hash: log.transaction_hash.unwrap(),
                        block_no: log.block_number.map(|b| b.as_u64()),
                    })
                    .with_context(|| {
                        anyhow!(
                            "Cannot find BlockProposed Tx {:?}",
                            log.transaction_hash.unwrap()
                        )
                    })?;
                let call = proposeBlockCall::abi_decode(&tx.input, false).unwrap();
                // .with_context(|| "failed to decode propose block call")?;
                return Ok((call, event));
            }
        }
        bail!("No BlockProposed event found for block {l2_block_no}");
    }
}
