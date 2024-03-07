#!/bin/bash

# Use the first command line argument as the chain name
chain="$1"
# Use the second command line argument as the proof type
proof="$2"
# Use the third(/fourth) parameter(s) as the block number as a range
rangeStart="$3"
rangeEnd="$4"

# Check the caain name and set the corresponding RPC values
if [ "$chain" == "testnet" ]; then
  l2Rpc="https://rpc.katla.taiko.xyz"
  l1Rpc="https://l1rpc.katla.taiko.xyz"
elif [ "$chain" == "internal_devnet_a" ]; then
  l2Rpc="https://rpc.internal.taiko.xyz"
  l1Rpc="https://l1rpc.internal.taiko.xyz"
else
  echo "Invalid chain name. Please use 'testnet' or 'internal_devnet_a'."
  exit 1
fi

if [ "$proof" == "native" ]; then
  proofType='"native"'
elif [ "$proof" == "risc0" ]; then
  proofType='{
    "risc0": {
      "bonsai": false,
      "snark": false,
      "profile": true,
      "execution_po2": 18
    }
  }'
else
  echo "Invalid proof name. Please use 'native' or 'risc0'."
  exit 1
fi

if [ "$rangeStart" == "" ]; then
  echo "Please specify a valid block range like \"10\" or \"10 20\""
  exit 1
fi

if [ "$rangeEnd" == "" ]; then
  rangeEnd=$rangeStart
fi

beaconRpc="https://l1beacon.internal.taiko.xyz"
prover="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
graffiti="8008500000000000000000000000000000000000000000000000000000000000"

for block in $(eval echo {$rangeStart..$rangeEnd});
do
  echo "- proving block $block"
  curl --location --request POST 'http://localhost:8080' \
       --header 'Content-Type: application/json' \
       --data-raw "{
         \"jsonrpc\": \"2.0\",
         \"id\": 1,
         \"method\": \"proof\",
         \"params\": [
           {
             \"chain\": \"$chain\",
             \"l2Rpc\": \"$l2Rpc\",
             \"l1Rpc\": \"$l1Rpc\",
             \"beaconRpc\": \"$beaconRpc\",
             \"proofType\": $proofType,
             \"blockNumber\": $block,
             \"prover\": \"$prover\",
             \"graffiti\": \"$graffiti\"
           }
         ]
       }"
  echo "\\n"
done