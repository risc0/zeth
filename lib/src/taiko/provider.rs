use alloc::{string::String};
use std::path::PathBuf;

use alloy_primitives::{Address};
use alloy_sol_types::{SolCall};
use anyhow::{anyhow, bail, ensure, Context, Result};
use ethers_core::types::{Block, Transaction, H256, U256, U64};


use zeth_primitives::{
    ethers::{from_ethers_h160, from_ethers_h256},
    transactions::{EthereumTransaction, TxEssence},
};

use super::{anchorCall, decode_anchor, proposeBlockCall, BlockProposed};
use crate::{
    consts::ChainSpec,
    host::provider::{
        new_provider, BlockQuery,
        ProofQuery, Provider, TxQuery,
    },
    taiko::consts::{check_anchor_signature, ANCHOR_GAS_LIMIT, GOLDEN_TOUCH_ACCOUNT},
};

pub struct TaikoProvider {
    pub l1_provider: Box<dyn Provider>,
    pub l2_provider: Box<dyn Provider>,
    pub l2_spec: Option<ChainSpec>,
    pub prover: Option<Address>,
    pub l1_contract: Option<Address>,
    pub l2_contract: Option<Address>,
    pub l1_signal_service: Option<Address>,
    pub l2_signal_service: Option<Address>,
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
            prover: None,
            l1_contract: None,
            l2_contract: None,
            l1_signal_service: None,
            l2_signal_service: None,
        })
    }

    pub fn with_l2_spec(mut self, spec: ChainSpec) -> Self {
        self.l2_spec = Some(spec);
        self
    }

    pub fn with_prover(mut self, prover: Address) -> Self {
        self.prover = Some(prover);
        self
    }

    pub fn with_contracts(
        mut self,
        f: impl FnOnce() -> Result<(Address, Address, Address, Address)>,
    ) -> Self {
        if let Ok((l1_contract, l2_contract, l1_signal_service, l2_signal_service)) = f() {
            self.l1_contract = Some(l1_contract);
            self.l2_contract = Some(l2_contract);
            self.l1_signal_service = Some(l1_signal_service);
            self.l2_signal_service = Some(l2_signal_service);
        }
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
    ) -> Result<(proposeBlockCall, BlockProposed)> {
        let logs = self.l1_provider.filter_event_log::<BlockProposed>(
            self.l1_contract.unwrap(),
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
        bail!("No BlockProposed event found for block {}", l2_block_no);
    }

    pub fn check_anchor_with_blocks<TX1, TX2>(
        &mut self,
        l1_block: &Block<TX1>,
        l2_parent_block: &Block<TX2>,
        anchor: anchorCall,
    ) -> Result<()> {
        // 1. check l2 parent gas used
        ensure!(
            l2_parent_block.gas_used == U256::from(anchor.parentGasUsed),
            "parentGasUsed mismatch"
        );

        // 2. check l1 signal root
        if let Some(l1_signal_service) = self.l1_signal_service {
            let proof = self.l1_provider.get_proof(&ProofQuery {
                block_no: l1_block.number.unwrap().as_u64(),
                address: l1_signal_service.into_array().into(),
                indices: Default::default(),
            })?;
            let signal_root = from_ethers_h256(proof.storage_hash);
            ensure!(signal_root == anchor.l1SignalRoot, "l1SignalRoot mismatch");
        } else {
            bail!("l1_signal_service not set");
        }

        // 3. check l1 block hash
        ensure!(
            l1_block.hash.unwrap() == H256::from(anchor.l1Hash.0),
            "l1Hash mismatch"
        );

        Ok(())
    }

    pub fn check_anchor_tx(
        &self,
        anchor: &Transaction,
        l2_block: &Block<Transaction>,
    ) -> Result<()> {
        let tx1559_type = U64::from(0x2);
        ensure!(
            anchor.transaction_type == Some(tx1559_type),
            "anchor transaction type mismatch"
        );

        let tx: EthereumTransaction = anchor
            .clone()
            .try_into()
            .context(anyhow!("failed to decode anchor transaction: {:?}", anchor))?;
        check_anchor_signature(&tx).context(anyhow!("failed to check anchor signature"));

        ensure!(
            from_ethers_h160(anchor.from) == *GOLDEN_TOUCH_ACCOUNT,
            "anchor transaction from mismatch"
        );
        ensure!(
            from_ethers_h160(anchor.to.unwrap()) == self.l1_contract.unwrap(),
            "anchor transaction to mismatch"
        );
        ensure!(
            anchor.value == U256::from(0),
            "anchor transaction value mismatch"
        );
        ensure!(
            anchor.gas == U256::from(ANCHOR_GAS_LIMIT),
            "anchor transaction gas price mismatch"
        );
        ensure!(
            anchor.max_fee_per_gas == l2_block.base_fee_per_gas,
            "anchor transaction gas mismatch"
        );

        Ok(())
    }
}
