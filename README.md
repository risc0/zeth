# zeth

NOTICE: Zeth has recently been revised to utilize [reth](https://github.com/paradigmxyz/reth) instead of just [revm](https://github.com/bluealloy/revm). Some features that may be mentioned in the rest of the readme, are still being re-added:
* Release builds

Zeth is an open-source ZK execution-layer block prover for Ethereum and Optimism built on the RISC Zero zkVM.

Zeth makes it possible to *prove* that a given block is valid
(i.e., is the result of applying the given list of transactions to the parent block)
*without* relying on the validator or sync committees.
This is because Zeth does *all* the work needed to construct a new block *from within the zkVM*, including:

* Verifying transaction signatures.
* Verifying account & storage state against the parent block’s state root.
* Applying transactions.
* Paying the block reward.
* Updating the state root.
* Etc.

After constructing the new block, Zeth calculates and outputs the block hash.
By running this process within the zkVM, we obtain a ZK proof that the new block is valid.

## Status

Zeth's block building logic uses reth 1.1.0, but its other components are not audited for use in production.

## Usage

### Prerequisites

Zeth primarily requires the availability of archival Ethereum/Optimism RPC provider(s) data.
Two complementary types of providers are supported:

* RPC provider.
  This fetches data from a Web2 RPC provider, such as [Alchemy](https://www.alchemy.com/), whose URL is specified using the `--rpc-url=<RPC_URL>` parameter.
* Cached RPC provider.
  This fetches RPC data from a local file when possible, and falls back to an RPC provider when necessary.
  It amends the local file with results from the RPC provider so that subsequent runs don't require additional RPC calls.
  Specified using the `--cache[=<CACHE>]` parameter.

### Installation

#### RISC Zero zkVM

Follow the installation steps for the RISC Zero zkVM:

https://dev.risczero.com/api/zkvm/install

**Note: v1.81 of the toolchain is required:**

```shell
cargo risczero build-toolchain --version r0.1.81.0-rc.4
```

#### zeth

Clone the repository and build with `cargo` using one of the following commands:

* CPU Proving (slow):
```shell
cargo build -p zeth-ethereum --bin zeth-ethereum
```
```shell
cargo build -p zeth-optimism --bin zeth-optimism
```

- GPU Proving (apple/metal)
```shell
cargo build -p zeth-ethereum --bin zeth-ethereum -F metal
```
```shell
cargo build -p zeth-optimism --bin zeth-optimism -F metal
```

- GPU Proving (nvidia/cuda)
```shell
cargo build -p zeth-ethereum --bin zeth-ethereum -F cuda
```
```shell
cargo build -p zeth-optimism --bin zeth-optimism -F cuda
```


#### Execution:

Run the built binary (instead of using `cargo run`) using:

```shell
RUST_LOG=info ./target/debug/zeth-ethereum
```
or for Optimism
```shell
RUST_LOG=info ./target/debug/zeth-optimism
```

Note that Optimism support is only available for post-Bedrock blocks.

### CLI

> Note: Usage for `zeth-optimism` is the same as that for `zeth-ethereum` as shown below. 

Zeth currently has four main modes of execution:

```shell
RUST_LOG=info ./target/debug/zeth-ethereum help
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
RUST_LOG=info ./target/debug/zeth-ethereum build --help
```

```console
Build blocks natively outside the RISC Zero zkVM

Usage: zeth build [OPTIONS] --block-number=<BLOCK_NUMBER>

Options:
  -r, --rpc-url=<RPC_URL>           URL of the execution-layer RPC node
  -c, --cache[=<CACHE>]             Cache RPC calls locally; the value specifies the cache directory [default when the flag is present: cache_rpc]
  -b, --block-number=<BLOCK_NUMBER> Starting block number
  -n, --block-count=<BLOCK_COUNT>   Number of blocks to build in a single proof [default: 1]
  -s, --chain=<CHAIN>               Which chain spec to use
```

When run in this mode, Zeth does all the work needed to construct an Ethereum block and verifies the correctness
of the result using the RPC provider.
No proofs are generated.

**Example**
The `bin/zeth-ethereum/data` directory comes preloaded with a few cache files that you can use
out of the box without the need to explicitly specify an RPC URL:
```shell
RUST_LOG=info ./target/debug/zeth-ethereum build \
  --cache=bin/zeth-ethereum/data \
  --block-number=1
```
Preloaded cache data is provided under `bin/zeth-ethereum/data` for all major Ethereum fork blocks:
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
RUST_LOG=info ./target/debug/zeth-ethereum run --help  
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
RUST_LOG=info ./target/debug/zeth-ethereum run \
  --cache=bin/zeth-ethereum/data \
  --block-number=1
```

#### prove
*This command generates a real ZK proof, unless dev mode is enabled through the environment variable `RISC0_DEV_MODE=1`.*

Generated proofs are saved locally as `.zkp` files (or `.fake` under dev mode).
```shell
RUST_LOG=info ./target/debug/zeth-ethereum prove --help
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
RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum prove \
  --cache=bin/zeth-ethereum/data \
  --block-number=1
```

***NOTE*** Proving in dev mode only generates dummy receipts that do not attest to the validity of the computation and
are not verifiable outside of dev mode! To generate a real cryptographic proof, do not set the `RISC0_DEV_MODE` environment variable.

#### verify
*This command verifies a ZK proof.*
```shell
RUST_LOG=info ./target/debug/zeth-ethereum verify --help
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
The below example will verify a fake receipt, where such verification can only pass under dev mode:
```shell
RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum verify \
  --cache=bin/zeth-ethereum/data \
  --block-number=1 \
  --file=risc0-1.1.2-0x0c5dda870412334fe6011ed1bfdc2b7f7a68b794b4fedc360c1c6db160096036.fake
```

***NOTE*** The aforementioned receipt will very likely have a different name on your machine.

## Additional resources

Check out these resources and say hi on our Discord:

* [RISC Zero developer’s portal](https://dev.risczero.com/)
* [zkVM quick-start guide](https://dev.risczero.com/zkvm/quickstart)
* [Bonsai quick-start guide](https://dev.risczero.com/bonsai/quickstart)
* [RISC Zero on Discord](https://discord.gg/risczero)
