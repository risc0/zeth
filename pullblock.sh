#!/bin/bash

for block in {10..10000..1}
do
  curl --location --request POST 'http://localhost:8080' \
       --header 'Content-Type: application/json' \
       --data-raw "{
         \"jsonrpc\": \"2.0\",
         \"id\": 1,
         \"method\": \"proof\",
         \"params\": [
           {
             \"type\": \"succinct\",
             \"l2Rpc\": \"https://rpc.katla.taiko.xyz\",
             \"l1Rpc\": \"https://ethereum-holesky-rpc.publicnode.com\",
             \"l2Contracts\": \"testnet\",
             \"proofInstance\": \"succinct\",
             \"blockNumber\": $block,
             \"prover\": \"0x70997970C51812dc3A010C7d01b50e0d17dc79C8\",
             \"graffiti\": \"0000000000000000000000000000000000000000000000000000000000000000\"
           }
         ]
       }"
done


# https://ethereum-holesky-rpc.publicnode.com 🉑️

# #!/bin/bash

# for block in {10..10000..1}
# do
#   curl --location --request POST 'http://localhost:8080' \
#        --header 'Content-Type: application/json' \
#        --data-raw "{
#          \"jsonrpc\": \"2.0\",
#          \"id\": 1,
#          \"method\": \"proof\",
#          \"params\": [
#            {
#              \"type\": \"risc0\",
#              \"l2Rpc\": \"https://rpc.katla.taiko.xyz\",
#              \"l1Rpc\": \"https://ethereum-holesky-rpc.publicnode.com\",
#              \"l2Contracts\": \"testnet\",
#              \"proofInstance\": {
#                \"risc0\": {
#                  \"bonsai\": false,
#                  \"snark\": false,
#                  \"profile\": true,
#                  \"execution_po2\": 18
#                }
#              },
#              \"blockNumber\": $block,
#              \"prover\": \"0x70997970C51812dc3A010C7d01b50e0d17dc79C8\",
#              \"graffiti\": \"0000000000000000000000000000000000000000000000000000000000000000\"
#            }
#          ]
#        }"
# done