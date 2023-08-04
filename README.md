# zeth

## git-lfs

To facilitate testing, this repository includes cached RPC data. These data are tracked using [git-lfs](https://git-lfs.com/). To use these files:

1. Install `git-lfs`.
2. Pull the cache files using `git lfs pull`.

This will fetch the cached RPC data and store the results in the `host/testdata` directory.

## Building

```console
$ cargo build --release
```

## Running

The `zeth` tool requires an Eth data provider. Three different providers are supported:

* File provider. Specified by giving `--cache-path FILENAME`.
* RPC provider. Specified by giving `--rpc-url RPC_URL`.
* Cached RPC provider. Specified by giving both `--cache-path FILENAME` and `--rpc-url RPC_URL`.

Example (replace `YOUR_API_KEY` with your API key for Alchemy):

```console
$ RUST_LOG=info ./target/release/zeth \
    --cache-path host/testdata \
    --block-no 16424130 \
    --rpc-url "https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY"
```

### Running in the zkVM executor

Add the flag `--local-exec (segment limit)`.

### Running on Bonsai

First, set the following environment variables:

* `BONSAI_API_URL`
* `BONSAI_API_KEY`

For example,

```console
$ export BONSAI_API_URL="bonsai_url"
$ export BONSAI_API_KEY="my_api_key"
```

To submit a proving job to Bonsai, add the flag `--bonsai-submit`. This will submit the job to Bonsai, print the session UUID to console, and poll Bonsai until the job is complete. For example,

```console
$ RUST_LOG=info ./target/release/zketh --cache-path host/testdata --block-no 17735424 --bonsai-submit
```

To check the status of a job that's already been submitted to Bonsai, add the flag `--bonsai-verify SESSION_UUID`, where `SESSION_UUID` is the UUID printed by the `--bonsai-submit` command. For example,

```console
$ RUST_LOG=info ./target/release/zketh --cache-path host/testdata --block-no 17735424 --bonsai-verify f150e1c6-ca9f-4c8f-9dfb-e9e022315e5c
```
