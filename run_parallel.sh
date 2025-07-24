#!/bin/bash
# Usage: ./script_name.sh [jobs]
# If not provided, the number of jobs defaults to 4.

# Set the number of jobs based on the first command-line argument (default 4)
JOBS="${1:-4}"

# Set the Ethereum RPC URL from the environment variable, with a default fallback
ETH_RPC_URL="${ETH_RPC_URL:-"https://ethereum-rpc.publicnode.com"}"

# Name of CSV output file
OUTPUT_CSV="results.csv"

# Write CSV header to the output file
echo "block_number,execution_time,total_cycles,user_cycles,paging_cycles,keccak_calls,gas_used" > "$OUTPUT_CSV"

# This function processes a single file: it extracts the block number,
# runs the command, extracts metrics from its output, and finally echoes a CSV row.
process_file() {
  file="$1"

  # The ETH_RPC_URL needs to be available in the sub-shell spawned by parallel
  ETH_RPC_URL="${ETH_RPC_URL:-"https://ethereum-rpc.publicnode.com"}"

  hex_to_dec() {
    local value="$1"
    if [[ "$value" =~ ^0x ]]; then
      echo $((value))
    else
      echo "$value"
    fi
  }

  # Extract the number from the filename: remove up to '_' and the '.json' suffix
  hash=${file#*_}
  hash=${hash%.json}

  # Log progress (sent to stderr so it won’t pollute the CSV output)
  echo "Processing file: $file with number: $hash" >&2

  # Run your command and capture both stdout and stderr
  output=$(
    RISC0_INFO=true RUST_LOG=info RISC0_DEV_MODE=true ./target/release/host --eth-rpc-url "$ETH_RPC_URL" --block "$hash" prove 2>&1
  )

  # echo command output to stderr for debugging purposes
  echo "$output" >&2

  # Extract the execution time by capturing both the numeric value and its unit.
  # This regex will match lines like:
  #   execution time: 1.23s
  #   execution time: 208.295537ms
  if [[ "$output" =~ execution\ time:\ ([0-9.]+)(ms|s) ]]; then
    time_number="${BASH_REMATCH[1]}"
    time_unit="${BASH_REMATCH[2]}"
    if [[ "$time_unit" == "ms" ]]; then
      # Convert milliseconds to seconds
      execution_time=$(awk "BEGIN {printf \"%.6f\", $time_number/1000}")
    else
      execution_time="$time_number"
    fi
  else
    execution_time="N/A"
  fi

  # Extract other metrics using grep with Perl-compatible regular expressions
  total_cycles=$(echo "$output" | perl -nE 'say $1 if /(\d+) total cycles/')
  user_cycles=$(echo "$output" | perl -nE 'say $1 if /(\d+) user cycles \(/')
  paging_cycles=$(echo "$output" | perl -nE 'say $1 if /(\d+) paging cycles \(/')
  keccak_calls=$(echo "$output" | perl -nE 'say $1 if /(\d+) Keccak calls/')

  # For robustness: if extraction fails, substitute with "N/A"
  total_cycles=${total_cycles:-"N/A"}
  user_cycles=${user_cycles:-"N/A"}
  paging_cycles=${paging_cycles:-"N/A"}
  keccak_calls=${keccak_calls:-"N/A"}

  # Retrieve the total gas used for the block using foundry cast.
  # The output is expected in JSON format, so we use jq to parse it.
  gas_used="N/A"
  block_json=$(cast block "$hash" --rpc-url "$ETH_RPC_URL" --format-json)
  block_number=$(hex_to_dec "$(echo "$block_json" | jq -r '.number')")
  gas_used=$(hex_to_dec "$(echo "$block_json" | jq -r '.gasUsed')")

  # Output the CSV row to stdout
  echo "$block_number,$execution_time,$total_cycles,$user_cycles,$paging_cycles,$keccak_calls,$gas_used"
}

# Export the function so that GNU Parallel's sub-shells can access it.
export -f process_file

# Use GNU Parallel with the configurable job count to process each file concurrently.
parallel --jobs "$JOBS" process_file ::: cache/input_0x*.json >> "$OUTPUT_CSV"
