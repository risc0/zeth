# Zether As a Service (ZaaS)

You can view the service here: 

This repo contains a full deployment of Risc0's Zeth. Components includes


- [Verification App]() - 
- [zeth]: I used a modified zeth instance, taking out the CLI (Clap), and converting it to a web service (actix). 
- [infrastructure]: With typescript / Pulumi , we deploy the front end and zeth to ECS , and use an Application Gateway for rate limiting and access control (i.e. we only want the front end interacting with zeth). 



- Create s3 backend 

PULUMI_BACKEND=s3://pulumi-state-sd

- Login to pulumi

```
pulumi login s3://pulumi-state-sd
```

- set PULUMI_CONFIG_PASSPHRASE 


## TODO


-API GATEWAY
- ECS
- Test
- Application
- Rate Limiting

**curl -X POST -H "Content-Type: application/json" -d '{
    "rpc_url": "https://eth-mainnet.g.alchemy.com/v2/zc3yCXhUS7G59c2YZrLtdEdYs8TpyjqQ",
    "network": "Ethereum",
    "block_no": 17999288,
    "submit_to_bonsai": false
}' "http://0.0.0.0:8000/verify"
**