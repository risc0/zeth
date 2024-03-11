# raiko

## Usage

### Building

- Install the `cargo risczero` tool and the `risc0` toolchain:

```console
$ cargo install cargo-risczero
$ cargo risczero install
```
- Install the `cargo prove` tool and the `succinct` toolchain:

```console
$ curl -L https://sp1.succinct.xyz | bash
$ sp1up
$ cargo prove --version
```


- Clone the repository and build with `cargo`:

```console
$ cargo build
```

### Running

Run the host in a terminal that will listen to requests:

Just for development with the native prover:
```
cargo run
```

Then in another terminal you can do requests like this:

```
./prove_block.sh testnet native 10
```

Look into `prove_block.sh` for the available options or run the script without inputs and it will tell you.

## Provers

Provers can be enabled using features. To compile with all of them (using standard options):

```
cargo run --release --features "risc0 succinct"
```

### risc zero
#### Testing
```
RISC0_DEV_MODE=1 cargo run --release --features risc0
```

#### CPU
```
cargo run --release --features risc0
```

#### GPU

```
RISC0_DEV_MODE=1 cargo run -F cuda --release --features risc0
```
OR
```
RISC0_DEV_MODE=1 cargo run -F metal --release --features risc0
```

CUDA needs to be installed when using `cuda`: https://docs.nvidia.com/cuda/cuda-installation-guide-linux/index.html

### succinct's SP1:
```
cargo run --release --features succinct
```