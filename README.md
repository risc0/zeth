# zeth

Zeth is an open-source ZK block prover for Ethereum and Optimism built on the RISC Zero zkVM.

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
For Optimism, our validity proof ensures that the block is correctly derived from the
available data posted to Ethereum.

## Status

Zeth is experimental and may still contain bugs.

## Usage

### Prerequisites

Zeth primarily requires the availability of Ethereum/Optimism RPC provider(s) data.
Two complementary types of providers are supported:

* RPC provider.
  This fetches data from a Web2 RPC provider, such as [Alchemy](https://www.alchemy.com/).
  Specified using the `--eth-rpc-url=<RPC_URL>` and `--op-rpc-url=<RPC_URL>` parameters.
* Cached RPC provider.
  This fetches RPC data from a local file when possible, and falls back to a Web2 RPC provider when necessary.
  It amends the local file with results from the Web2 provider so that subsequent runs don't require additional Web2 RPC calls.
  Specified using the `--cache[=<CACHE>]` parameter.

### Installation


#### RISC Zero zkVM

Install the `cargo risczero` tool and the `risc0` toolchain:

```console
cargo install cargo-risczero
cargo risczero install
```

#### zeth

Clone the repository and build with `cargo` using one of the following commands:

* CPU Proving (slow):
```console
cargo build --release
```

- GPU Proving (apple/metal)
```console
cargo build -F metal --release
```

- GPU Proving (nvidia/cuda)
```console
cargo build -F cuda --release
```

#### docker (recommended)

If you wish to use the `--release` profile when building Zeth,
check out https://docs.docker.com/engine/install/ for a guide on how to install docker, which is required for reproducible builds of the zkVM binaries in Zeth.


#### Execution:

Run the built binary (instead of using `cargo run`) using:

```console
RUST_LOG=info ./target/release/zeth
```

### CLI

Zeth currently has four main modes of execution:

```console
RUST_LOG=info ./target/release/zeth help
```
```console
Usage: zeth <COMMAND>

Commands:
  build    Build blocks natively outside the zkVM
  run      Run the block creation process inside the executor
  prove    Provably create blocks inside the zkVM
  verify   Verify a block creation receipt
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

For every command, the `--network` parameter can be set to either `ethereum` or `optimism` for provable construction
of single blocks from either chain on its own.
To provably derive Optimism blocks using the data posted on the Ethereum chain, use `--network=optimism-derived`,
but `optimism-derived` is not supported by the `run` commands.

#### build
*This command only natively builds blocks and does not generate any proofs.*
```console
RUST_LOG=info ./target/release/zeth build --help
```

```console
Build blocks natively outside the zkVM

Usage: zeth build [OPTIONS] --block-number=<BLOCK_NUMBER>

Options:
  -w, --network=<NETWORK>            Network name (ethereum/optimism/optimism-derived) [default: ethereum]
  -e, --eth-rpc-url=<ETH_RPC_URL>    URL of the Ethereum RPC node
  -o, --op-rpc-url=<OP_RPC_URL>      URL of the Optimism RPC node
  -c, --cache[=<CACHE>]              Use a local directory as a cache for RPC calls. Accepts a custom directory. [default: cache_rpc]
  -b, --block-number=<BLOCK_NUMBER>  Block number to begin from
  -n, --block-count=<BLOCK_COUNT>    Number of blocks to provably derive [default: 1]
  -m, --composition[=<COMPOSITION>]  Compose separate block derivation proofs together. Accepts a custom number of blocks to process per derivation call. (optimism-derived network only) [default: 1]
  -h, --help                         Print help
```

When run in this mode, Zeth does all the work needed to construct an Ethereum block and verifies the correctness
of the result using the RPC provider.
No proofs are generated.

With `--network=optimism-derived`, the derivation proof creation is done without proof composition by default,
requiring the derivation to be carried out inside a single zkVM execution.

**Examples**
The `host/testdata` and `host/testdata/derivation` directories come preloaded with a few cache files that you can use
out of the box without the need to explicitly specify an RPC URL:
```console
RUST_LOG=info ./target/release/zeth build \
  --network=ethereum \
  --cache=host/testdata \
  --block-number=16424130
```
```console
RUST_LOG=info ./target/release/zeth build \
  --network=optimism \
  --cache=host/testdata \
  --block-number=107728767
```
```console
RUST_LOG=info ./target/release/zeth build \
  --network=optimism-derived \
  --cache=host/testdata/derivation \
  --block-number=109279674 \
  --block-count=4
```
**Composition** The optimism derivation proof (`--network=optimism-derived`) can alternatively be created using proof composition by
setting the `--composition` parameter to the number of op blocks per rolled up proof.
In the following example, 2 derivation proofs of 2 sequential blocks each are composed to obtain the final derivation
proof for the 4 sequential blocks:
```console
RUST_LOG=info ./target/release/zeth build \
  --network=optimism-derived \
  --cache=host/testdata/derivation \
  --block-number=109279674 \
  --block-count=4 \
  --composition=2
```

#### run
*This command only invokes the RISC-V emulator and does not generate any proofs.*
```console
RUST_LOG=info ./target/release/zeth run --help  
```
```console
Run the block creation process inside the executor

Usage: zeth run [OPTIONS] --block-number=<BLOCK_NUMBER>

Options:
  -w, --network=<NETWORK>            Network name (ethereum/optimism/optimism-derived) [default: ethereum]
  -e, --eth-rpc-url=<ETH_RPC_URL>    URL of the Ethereum RPC node
  -o, --op-rpc-url=<OP_RPC_URL>      URL of the Optimism RPC node
  -c, --cache[=<CACHE>]              Use a local directory as a cache for RPC calls. Accepts a custom directory. [default: cache_rpc]
  -b, --block-number=<BLOCK_NUMBER>  Block number to begin from
  -n, --block-count=<BLOCK_COUNT>    Number of blocks to provably derive [default: 1]
  -x, --execution-po2=<LOCAL_EXEC>      The maximum segment cycle count as a power of 2 [default: 20]
  -p, --profile                      Whether to profile the zkVM execution
  -h, --help                         Print help
```

**Local executor mode**.
When run in this mode, Zeth does all the work needed to construct an Ethereum block from within the zkVM's non-proving emulator.
Correctness of the result is checked using the RPC provider.
This is useful for measuring the size of the computation (number of execution segments and cycles).
No proofs are generated.

**Examples**
The below examples will invoke the executor, which will take a bit more time, and output the number of cycles required
for execution/proving inside the zkVM:
```console
RUST_LOG=info ./target/release/zeth run \
  --cache=host/testdata \
  --network=ethereum \
  --block-number=16424130
```
```console
RUST_LOG=info ./target/release/zeth run \
  --cache=host/testdata \
  --network=optimism \
  --block-number=107728767
```

The `run` command does not support proof composition (required by `--network=optimism-derived`) because receipts are required for this process inside the
executor.
Alternatively, one can call the `prove` command in dev mode (`RISC0_DEV_MODE=true`) for the same functionality, as
demonstrated in the next section.

#### prove
*This command generates a ZK proof, unless dev mode is enabled through the environment variable `RISC0_DEV_MODE=true`.*
```console
RUST_LOG=info ./target/release/zeth prove --help
```
```console
Provably create blocks inside the zkVM

Usage: zeth prove [OPTIONS] --block-number=<BLOCK_NUMBER>

Options:
  -w, --network=<NETWORK>            Network name (ethereum/optimism/optimism-derived) [default: ethereum]
  -e, --eth-rpc-url=<ETH_RPC_URL>    URL of the Ethereum RPC node
  -o, --op-rpc-url=<OP_RPC_URL>      URL of the Optimism RPC node
  -c, --cache[=<CACHE>]              Use a local directory as a cache for RPC calls. Accepts a custom directory. [default: cache_rpc]
  -b, --block-number=<BLOCK_NUMBER>  Block number to begin from
  -n, --block-count=<BLOCK_COUNT>    Number of blocks to provably derive [default: 1]
  -x, --execution-po2=<LOCAL_EXEC>      The maximum segment cycle count as a power of 2 [default: 20]
  -p, --profile                      Whether to profile the zkVM execution
  -m, --composition[=<COMPOSITION>]  Compose separate block derivation proofs together. Accepts a custom number of blocks to process per derivation call. (optimism-derived network only) [default: 1]
  -s, --submit-to-bonsai             Prove remotely using Bonsai
  -h, --help                         Print help
```

**Proving on Bonsai**.
To run in this mode, add the parameter `--submit-to-bonsai`.
When run in this mode, Zeth submits a proving task to the [Bonsai proving service](https://www.bonsai.xyz/),
which then constructs the blocks entirely from within the zkVM.
This mode checks the correctness of the result on your machine using the RPC provider(s).
It also outputs the Bonsai session UUID, and polls Bonsai until the proof is complete.

To use this feature, first set the `BONSAI_API_URL` and `BONSAI_API_KEY` environment variables before executing zeth
to submit jobs to Bonsai.

Need a Bonsai API key? [Sign up today](https://bonsai.xyz/apply).

**Examples**
The below examples will invoke the prover, which will take a potentially significant time to generate a ZK proof
locally:
```console
RUST_LOG=info ./target/release/zeth prove \
  --cache=host/testdata \
  --network=ethereum \
  --block-number=16424130
```
```console
RUST_LOG=info ./target/release/zeth prove \
  --cache=host/testdata \
  --network=optimism \
  --block-number=107728767
```
```console
RUST_LOG=info ./target/release/zeth prove \
  --network=optimism-derived \
  --cache=host/testdata/derivation \
  --block-number=109279674 \
  --block-count=4
```
**Composition** Alternatively, we can run composition in dev mode, which should only as much time as required by the
executor, using the following command:
```console
RISC0_DEV_MODE=true RUST_LOG=info ./target/release/zeth prove \
  --network=optimism-derived \
  --cache=host/testdata/derivation \
  --block-number=109279674 \
  --block-count=4 \
  --composition=2
```
***NOTE*** Proving in dev mode only generates dummy receipts that do not attest to the validity of the computation and
are not verifiable outside of dev mode!

#### verify
*This command verifies a ZK proof generated on Bonsai.*
```
RUST_LOG=info ./target/release/zeth verify --help  
```
```
Verify a block creation receipt

Usage: zeth verify [OPTIONS] --block-number=<BLOCK_NUMBER> --bonsai-receipt-uuid=<BONSAI_RECEIPT_UUID>

Options:
  -w, --network=<NETWORK>
          Network name (ethereum/optimism/optimism-derived) [default: ethereum]
  -e, --eth-rpc-url=<ETH_RPC_URL>
          URL of the Ethereum RPC node
  -o, --op-rpc-url=<OP_RPC_URL>
          URL of the Optimism RPC node
  -c, --cache[=<CACHE>]
          Use a local directory as a cache for RPC calls. Accepts a custom directory. [default: cache_rpc]
  -b, --block-number=<BLOCK_NUMBER>
          Block number to begin from
  -n, --block-count=<BLOCK_COUNT>
          Number of blocks to provably derive [default: 1]
  -b, --bonsai-receipt-uuid=<BONSAI_RECEIPT_UUID>
          Verify the receipt from the provided Bonsai Session UUID
  -h, --help
          Print help
```

This command first natively builds the specified block(s), and then validates the correctness of the receipt generated
on Bonsai specified by the `--bonsai-receipt-uuid=BONSAI_SESSION_UUID` parameter, where `BONSAI_SESSION_UUID` is the
session UUID returned when proving using `--submit-to-bonsai`.

## Additional resources

Check out these resources and say hi on our Discord:

* [RISC Zero developer’s portal](https://dev.risczero.com/)
* [zkVM quick-start guide](https://dev.risczero.com/zkvm/quickstart)
* [Bonsai quick-start guide](https://dev.risczero.com/bonsai/quickstart)
* [RISC Zero on Discord](https://discord.gg/risczero)
