default:
    just --summary --unsorted

build +ARGS="--release":
    cargo build -p zeth-ethereum --bin zeth-ethereum {{ARGS}}

    cargo build -p zeth-optimism --bin zeth-optimism {{ARGS}}

    cargo build -p zeth-benchmark --bin zeth-benchmark {{ARGS}}

cuda: (build "--release -F cuda")

metal: (build "--release -F metal")

run bin +ARGS:
    RUST_LOG=info ./target/release/zeth-{{bin}} {{ARGS}}

ethereum +ARGS: (run "ethereum" ARGS)

optimism +ARGS: (run "optimism" ARGS)

benchmark +ARGS: (run "benchmark" ARGS)

clippy:
    RISC0_SKIP_BUILD=1 cargo clippy -p zeth-ethereum

    RISC0_SKIP_BUILD=1 cargo clippy -p zeth-optimism

    RISC0_SKIP_BUILD=1 cargo clippy -p zeth-benchmark

    RISC0_SKIP_BUILD=1 cargo clippy -p zeth-testeth

test:
    cargo test --all-targets -p zeth-core -p zeth-preflight -p zeth-guests -p zeth -p zeth-benchmark -F debug-guest-build

    cargo test --all-targets -p zeth-core-ethereum -p zeth-preflight-ethereum -p zeth-ethereum -F debug-guest-build

    cargo test --all-targets -p zeth-core-optimism -p zeth-preflight-optimism -p zeth-optimism -F debug-guest-build

    cargo test --all-targets -p zeth-testeth -F ef-tests

    just test-cache-eth

test-cache-eth: (build "")
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=1
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=1150000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=1920000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=2463000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=2675000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=4370000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=7280000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=9069000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=9200000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=12244000
    # RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=12965000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=13773000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=15050000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=15537394
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=17034870
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/ethereum/data -b=19426587

test-cache-op: (build "")
    RUST_LOG=info ./target/debug/zeth-optimism build --cache=bin/optimism/data -c=optimism-sepolia -b=17664000