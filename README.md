# zeth

Zeth is an open-source ZK execution-layer block prover for Ethereum and Optimism built on the RISC Zero zkVM.

Zeth makes it possible to *prove* that a given sequence of blocks is valid
(i.e., is the result of applying the given lists of transactions starting from the parent block)
*without* relying on the validator or sync committees.
This is because Zeth does *all* the work needed to execute blocks *from within the zkVM*, including:

* Verifying transaction signatures.
* Verifying account & storage state against the parent block’s state root.
* Applying transactions.
* Paying the block reward.
* Updating the state root.
* Etc.

By using reth to run the block execution process within the zkVM, we obtain a ZK proof of valid block execution.

## Status

Zeth uses version `1.2.1-rc.1` of the RISC Zero zkVM and version 1.1.0 of reth (backed by revm 14.0.3), but its other components are not audited for use in production.

## Prerequisites
1. [rust](https://www.rust-lang.org/tools/install)
2. [just](https://just.systems/man/en/)
3. [docker](https://www.docker.com/)

## Usage

### Requirements

Zeth primarily requires the availability of archival Ethereum/Optimism RPC provider(s) data.
Two complementary types of providers are supported:

* RPC provider.
  This fetches data from a Web2 RPC provider, such as [Alchemy](https://www.alchemy.com/), whose URL is specified using the `--rpc=<RPC>` parameter.
* Cached RPC provider.
  This fetches RPC data from a local file when possible, and falls back to an RPC provider when necessary.
  It amends the local file with results from the RPC provider so that subsequent runs don't require additional RPC calls.
  Specified using the `--cache[=<CACHE>]` parameter.

### Installation

#### RISC Zero zkVM

Follow the installation steps for the RISC Zero zkVM:

https://dev.risczero.com/api/zkvm/install

**Note: At least v1.1.3 is required:**


#### zeth

Clone the repository and build using one of the following commands:

* CPU Proving (slow):
```shell
just build
```

- GPU Proving (apple/metal)
```shell
just metal
```

- GPU Proving (nvidia/cuda)
```shell
just cuda
```

#### Execution:

Run the built binary (instead of using `cargo run`) using:

```shell
just ethereum
```
or for Optimism
```shell
just optimism
```

Note that Optimism support is only available for post-Bedrock blocks.

### CLI

> Note: Usage for `zeth-optimism` is the same as that for `zeth-ethereum` as shown below. 

Zeth currently has four main modes of execution:

```shell
just ethereum help
```
```console
Usage: zeth <COMMAND>

Commands:
  build   Build blocks only on the host
  run     Run the block building inside the executor
  prove   Provably build blocks inside the zkVM
  verify  Verify a block building receipt
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

#### build
*This command only natively builds blocks and does not generate any proofs.*
```shell
just ethereum build --help
```

```console
Build blocks natively outside the RISC Zero zkVM

Usage: zeth build [OPTIONS] --block-number=<BLOCK_NUMBER>

Options:
  -r, --rpc=<RPC>           URL of the execution-layer RPC node
  -c, --cache[=<CACHE>]             Cache RPC calls locally; the value specifies the cache directory [default when the flag is present: cache_rpc]
  -b, --block-number=<BLOCK_NUMBER> Starting block number
  -n, --block-count=<BLOCK_COUNT>   Number of blocks to build in a single proof [default: 1]
  -s, --chain=<CHAIN>               Which chain spec to use
```

When run in this mode, Zeth does all the work needed to construct an Ethereum block and verifies the correctness
of the result using the RPC provider.
No proofs are generated.

**Example**
The `bin/ethereum/data` directory comes preloaded with a few cache files that you can use
out of the box without the need to explicitly specify an RPC URL:
```shell
just ethereum build \
  --cache=bin/ethereum/data \
  --block-number=1
```
Preloaded cache data is provided under `bin/ethereum/data` for all major Ethereum fork blocks:
```
Block     Fork
1         Frontier
1150000   Homestead
1920000   Dao
2463000   Tangerine
2675000   Spurious Dragon
4370000   Byzantium
7280000   Constantinople / Petersburg
9069000   Istanbul
9200000   Muir Glacier
12244000  Berlin
12965000  London
13773000  Arrow Glacier
15050000  Gray Glacier
15537394  Paris / Merge
17034870  Shanghai
19426587  Dencun
```

When no RPC URL is given, the `--chain` parameter defines which chain fork is being used, with network's mainnet chain being the default when the parameter and the RPC URL are missing.
The supported chains are listed below:

`zeth-ethereum`:
```
mainnet           (Ethereum mainnet)
sepolia           (Ethereum Sepolia testnet)
holesky           (Ethereum Holesky testnet)
dev               (Local Ethereum devnet)
```
`zeth-optimism`:
```
optimism          (Optimism mainnet)
optimism-sepolia  (Optimism Sepolia testnet)
base              (Base mainnet)
base-sepolia      (Base Sepolia testnet)
dev               (Local Optimism devnet)
```

#### run
*This command only invokes the RISC Zero executor and does not generate any proofs.*
```shell
just ethereum run --help  
```
```console
Build blocks inside the RISC Zero zkVM executor

Usage: zeth run [OPTIONS] --block-number=<BLOCK_NUMBER>

Options:
  -x, --execution-po2=<EXECUTION_PO2> The maximum cycle count of a segment as a power of 2 [default: 20]
```

**Local executor mode**.
When run in this mode, Zeth does all the work needed to construct an Ethereum block from within the zkVM's non-proving emulator.
Correctness of the result is checked using the RPC provider.
This is useful for measuring the size of the computation (number of execution segments and cycles).
No proofs are generated.

**Example**
The below example will invoke the executor, which will take a bit more time, and output the number of cycles required
for execution/proving inside the zkVM:
```shell
just ethereum run \
  --cache=bin/ethereum/data \
  --block-number=1
```

#### prove
*This command generates a real ZK proof, unless dev mode is enabled through the environment variable `RISC0_DEV_MODE=1`.*

Generated proofs are saved locally as `.zkp` files (or `.fake` under dev mode).
```shell
just ethereum prove --help
```
```console
Provably build blocks inside the RISC Zero zkVM

Usage: zeth prove [OPTIONS] --block-number=<BLOCK_NUMBER>

Options:
  -s, --snark Convert the resulting STARK receipt into a Groth-16 SNARK
```


**Proving on Bonsai**.
To use this feature, first set the `BONSAI_API_URL` and `BONSAI_API_KEY` environment variables before executing zeth
to submit jobs to Bonsai.
With said environment variables set, Zeth submits a proving task to the [Bonsai proving service](https://www.bonsai.xyz/),
which then constructs the blocks entirely from within the zkVM.
This mode checks the correctness of the result on your machine using the RPC provider(s).
It waits for Bonsai until the proof is complete, and saves the receipt locally on your machine.

Need a Bonsai API key? [Sign up today](https://bonsai.xyz/apply).

**Example**
The below example will invoke the prover under dev mode, which will execute quickly and generate a fake receipt locally:
```shell
RISC0_DEV_MODE=1 just ethereum prove \
  --cache=bin/ethereum/data \
  --block-number=1
```

***NOTE*** Proving in dev mode only generates dummy receipts that do not attest to the validity of the computation and
are not verifiable outside of dev mode! To generate a real cryptographic proof, do not set the `RISC0_DEV_MODE` environment variable.

#### verify
*This command verifies a ZK proof.*
```shell
just ethereum verify --help
```
```console
Verify a block building proof

Usage: zeth verify [OPTIONS] --block-number=<BLOCK_NUMBER> --file=<FILE>

Options:
  -f, --file=<FILE> Receipt file path
```

This command first locally fetches some metadata about the specified block(s) to build the expected receipt journal,
and then validates the correctness of the specified receipt file.

**Example**
The below example will verify the generated fake receipt, where such verification can only pass under dev mode:
```shell
RISC0_DEV_MODE=1 just ethereum verify \
  --cache=bin/ethereum/data \
  --block-number=1 \
  --file=risc0-1.1.3-0x02b54eb99985f0c6728b42adc41b0e472ead6ca0c2bd8ff9bb96352182f94331.fake
```

## Additional resources

Check out these resources and say hi on our Discord:

* [RISC Zero developer’s portal](https://dev.risczero.com/)
* [zkVM quick-start guide](https://dev.risczero.com/zkvm/quickstart)
* [Bonsai quick-start guide](https://dev.risczero.com/bonsai/quickstart)
* [RISC Zero on Discord](https://discord.gg/risczero)
