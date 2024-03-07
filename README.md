# raiko

## Usage

### Building

- Install the `cargo risczero` tool and the `risc0` toolchain:

```console
$ cargo install cargo-risczero
$ cargo risczero install
```

- Clone the repository and build with `cargo`:

```console
$ cargo build
```

### Running

Run the host in a terminal that will listen to requests:

```
RISC0_DEV_MODE=1 cargo run
```

Then in another terminal you can do requests like this:

```
./prove_block.sh testnet risc0 10
```

Look into `prove_block.sh` for the available options or run the script without inputs and it will tell you.