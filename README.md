# zeth

Zeth is an open-source ZK block prover for Ethereum built on the RISC Zero zkVM.

Zeth makes it possible to *prove* that a given Ethereum block is valid (i.e., is the result of applying the given list of transactions to the parent block) *without* relying on the validator or sync committees. This is because Zeth does *all* of the work needed to construct a new block *from within the zkVM*, including:

* Verifying transaction signatures.
* Verifying account & storage state against the parent block’s state root.
* Applying transactions.
* Paying the block reward.
* Updating the state root.
* Etc.

After constructing the new block, Zeth calculates and outputs its hash. By running this process within the zkVM, we obtain a ZK proof that the new block is valid.

## Status

Zeth is experimental and may still contain bugs.

## Usage

### Building

Clone the repository and build with `cargo`:

```console
$ cargo build --release
```

### Configuration

Zeth requires an Ethereum RPC provider. Three different providers are supported:

* File provider. This fetches RPC data from a local file. Specified by giving `--cache-path FILENAME`.
* RPC provider. This fetches data from a Web2 RPC provider, such as [Alchemy](https://www.alchemy.com/). Specified by giving `--rpc-url RPC_URL`.
* Cached RPC provider. This fetches RPC data from a local file when possible, and falls back to a Web2 RPC provider when necessary. It also updates the local file with results from the Web2 provider so that subsequent runs don't need to make any additional Web2 RPC calls. Specified by giving both `--cache-path FILENAME` and `--rpc-url RPC_URL`.

For proving, Zeth has built-in support for the [Bonsai proving service](https://www.bonsai.xyz/). To use this feature, first set the `BONSAI_API_URL` and `BONSAI_API_KEY` environment variables, then follow the instructions below for submitting jobs to Bonsai and verifying the proofs.

Need a Bonsai API key? [Sign up today](https://bonsai.xyz/apply).

### Running

Zeth currently has several modes of execution:

**Quick test mode**. This is the default. When run in this mode, Zeth does all of the work needed to construct an Ethereum block and verifies the correctness of the result using the RPC provider. No proofs are generated.

```console
$ RUST_LOG=info ./target/release/zeth \
    --rpc-url "https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY" \
    --cache-path host/testdata \
    --block-no 16424130
```

**Local executor mode**. To run in this mode, add the flag `--local-exec SEGMENT_LIMIT`. When run in this mode, Zeth does all of the work needed to construct an Ethereum block from within the zkVM's non-proving emulator. Correctness of the result is checked using the RPC provider. This is useful for measuring the size of the computation (number of execution segments and cycles). No proofs are generated.

```console
$ RUST_LOG=info ./target/release/zeth \
    --rpc-url "https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY" \
    --cache-path host/testdata \
    --block-no 16424130 \
    --local-exec 20
```

**Bonsai proving mode**. *This mode generates a ZK proof.* To run in this mode, add the flag `--bonsai-submit`. When run in this mode, Zeth submits a proving task to Bonsai, which then constructs an Ethereum block entirely from within the zkVM. This mode checks the correctness of the result using the RPC provider. It also outputs the Bonsai session UUID, and polls Bonsai until the proof is complete.

```console
$ RUST_LOG=info ./target/release/zeth \
    --rpc-url "https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY" \
    --cache-path host/testdata \
    --block-no 16424130 \
    --bonsai-submit
```

**Bonsai verify mode**. *This mode verifies the ZK proof.* To run in this mode, add the flag `--bonsai-verify BONSAI_SESSION_UUID`, where `BONSAI_SESSION_UUID` is the session UUID returend by the `--bonsai-submit` mode. This mode checks the correctness of the result using the RPC provider.

```console
$ RUST_LOG=info ./target/release/zeth \
    --rpc-url "https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY" \
    --cache-path host/testdata \
    --block-no 16424130 \
    --bonsai-verify BONSAI_SESSION_UUID
```

## Additional resources

Check out these resources and say hi on our Discord:

* [RISC Zero developer’s portal](https://dev.risczero.com/)
* [zkVM quick-start guide](https://dev.risczero.com/zkvm/quickstart)
* [Bonsai quick-start guide](https://dev.risczero.com/bonsai/quickstart)
* [RISC Zero on Discord](https://discord.gg/risczero)
