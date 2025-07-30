# Zeth: A RISC Zero zkVM Block Prover for Ethereum

Zeth is an open-source, execution-layer block prover for Ethereum, built on the RISC Zero zkVM and leveraging the power of [Reth](https://reth.rs/).

By using Reth's stateless execution capabilities within the zkVM, Zeth makes it possible to generate a cryptographic proof of a block's validity. This allows you to verify the entire block execution process—from transaction validation to state root updates—without trusting a third-party node provider.

## How It Works

Zeth streamlines the process of proving block execution by taking advantage of reth's stateless execution feature. The core logic involves:

1. **Fetching Data**: Zeth requires an archival Ethereum RPC provider to fetch the block header and an "execution witness." The witness contains all the necessary pre-state data (account info, storage slots, bytecodes) required to execute the block from scratch.
2. **Stateless Execution**: The execution witness and the block data are provided as inputs to the RISC Zero zkVM.
3. **Proving**: Inside the zkVM, the guest program uses reth's stateless validation function to execute all transactions in the block, apply rewards, and compute the final state root.
4. **Journal Output**: The guest program commits the calculated state root to the public journal. The host can then verify this against the known, correct state root from the block header.

## Prerequisites

You'll need the following installed to run Zeth:

1. [Rust](https://www.rust-lang.org/tools/install) (see rust-toolchain.toml for the exact version).
2. The [RISC Zero toolchain](https://dev.risczero.com/api/zkvm/install).

## Building the Project

To build the host CLI and guest programs, run:
```bash
cargo build --release
```

### Deterministic Guest Builds with Docker

For ZK proofs, it is critical that the guest binary is built deterministically. This ensures that every developer builds the exact same guest program, resulting in the same Image ID, which is essential for verification.

This project is configured to use Docker to achieve reproducible builds. If you have Docker installed, you can enable this feature by setting an environment variable:
```bash
RISC0_USE_DOCKER=1 cargo build --release
```
This will build the guest programs inside a controlled Docker environment, guaranteeing a deterministic output. The project's release workflow uses this method to build the official guest binaries.

## Usage

Zeth requires an archival Ethereum RPC provider to fetch block and state data. You can provide this using the `ETH_RPC_URL` environment variable or the `--eth-rpc-url` command-line argument.

### RPC Provider Requirements

Zeth's core functionality depends on the non-standard `debug_executionWitness` RPC method to generate the execution witness. Not all RPC providers support this method.
If your provider does not support `debug_executionWitness`, you can use the included `zeth-rpc-proxy`. This proxy server will intercept requests, generate the witness locally using a standard archival node, and forward all other requests to your provider. You can run it with:

```bash
ETH_RPC_URL="<YOUR_ARCHIVAL_RPC_URL>" cargo run --release --bin zeth-rpc-proxy
```

You can then point the Zeth CLI to the proxy, which runs on `127.0.0.1:8545` by default.

### CLI Commands

The CLI provides two main commands for interacting with blocks.

```
$ cargo run --bin cli -- --help

Simple CLI to create Ethereum block execution proofs

Usage: cli [OPTIONS] --eth-rpc-url <ETH_RPC_URL> <COMMAND>

Commands:
  prove     Validate the block and generate a RISC Zero proof
  validate  Validate the block on the host machine, without proving
  help      Print this message or the help of the given subcommand(s)

Options:
      --eth-rpc-url <ETH_RPC_URL>  URL of the Ethereum RPC endpoint to connect to [env: ETH_RPC_URL=]
      --block <BLOCK>              Block number, tag, or hash (e.g., "latest", "0x1565483") to execute [default: latest]
      --cache-dir <CACHE_DIR>      Cache folder for input files [default: ./cache]
  -h, --help                       Print help
  -V, --version                    Print version
```

### `validate`

This command runs the block execution logic on your local machine (the "host") without generating a proof. It's useful for quickly verifying that a block can be correctly processed and for populating the local cache.

#### Validate the latest block
```bash
ETH_RPC_URL="<YOUR_RPC_URL>" cargo run --release --bin cli -- validate
```

#### Validate a specific block by number
```bash
ETH_RPC_URL="<YOUR_RPC_URL>" cargo run --release --bin cli -- validate
```

Upon first run, this will fetch the necessary data from the RPC and save it to the cache/ directory. Subsequent runs for the same block will be much faster as they will use the cached data.

### `prove`

This command first validates the block on the host and then proceeds to generate a full cryptographic proof of execution inside the RISC Zero zkVM.

#### Prove the latest block  
```bash
ETH_RPC_URL="<YOUR_RPC_URL>" cargo run --release --bin cli -- prove
```

**Developer Mode**: For faster iteration during development, you can generate a "mock" proof by enabling RISC Zero's dev mode. This runs the guest code in the zkVM but skips the expensive proof generation step.

#### Generate a mock proof for a specific block
```bash
RISC0_DEV_MODE=1 ETH_RPC_URL="<YOUR_RPC_URL>" cargo run --release --bin cli -- prove 19000000
```

## Additional Resources

* [RISC Zero Developer Portal](https://dev.risczero.com/)
* [reth \- The Rust Ethereum Book](https://reth.rs/)
* Say hi on the [RISC Zero Discord](https://discord.gg/risczero)
