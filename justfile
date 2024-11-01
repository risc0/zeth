build:
    cargo build -p zeth-ethereum --bin zeth-ethereum

    cargo build -p zeth-optimism --bin zeth-optimism

clippy:
    RISC0_SKIP_BUILD=1 cargo clippy -p zeth-ethereum

    RISC0_SKIP_BUILD=1 cargo clippy -p zeth-optimism

test-local-fast:
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=1
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=1150000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=1920000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=2463000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=2675000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=4370000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=7280000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=9069000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=9200000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=12244000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=12965000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=13773000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=15050000
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=15537394
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=17034870
    RUST_LOG=info RISC0_DEV_MODE=1 ./target/debug/zeth-ethereum build --cache=bin/zeth-ethereum/data -b=19426587