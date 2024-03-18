export BONSAI_API_KEY="1234"
export BONSAI_API_URL="https://api.bonsai.xyz/"
# https://dev.risczero.com/api/blockchain-integration/contracts/verifier
# RiscZeroGroth16Verifier.sol Sepolia	0x83C2e9CD64B2A16D3908E94C7654f3864212E2F8
# export GROTH16_VERIFIER_ADDRESS="83C2e9CD64B2A16D3908E94C7654f3864212E2F8"
export GROTH16_VERIFIER_ADDRESS="850EC3780CeDfdb116E38B009d0bf7a1ef1b8b38"
# use your own if sth wrong due to infura limits.
# export GROTH16_VERIFIER_RPC_URL="https://sepolia.infura.io/v3/4c76691f5f384d30bed910018c28ba1d"
export GROTH16_VERIFIER_RPC_URL="https://l1rpc.internal.taiko.xyz"
#export CC=gcc
#export CC_riscv32im_risc0_zkvm_elf=/opt/riscv/bin/riscv32-unknown-elf-gcc 
RUST_LOG="[executor]=info" cargo run --release --features risc0
