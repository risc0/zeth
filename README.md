# Zeth As a Service (ZaaS)

You can view the service [here](http://net-lb-bd44876-1703206818.us-east-1.elb.amazonaws.com:3000/): 

This repo contains a full deployment of Risc0's Zeth. Component includes:

- **verification_app** - Simple react form to pass data to zeth.
- **zeth**: I used a modified zeth instance, taking out the CLI (Clap), and converting it to a web service (actix). 
- **infrastructure** : With typescript / Pulumi , we deploy the front end and zeth to ECS , and use an Application Gateway for rate limiting and access control (i.e. we only want the front end interacting with zeth). 

## Running

- The entire stack is deployed with [Pulumi](https://www.pulumi.com/docs/). It assumes that the repository is cloned at `$HOME/zaas`. 
- In order to run configuration checks and set up pulumi backend, run `make setup` from the root directory.
- Once the previous step is completed, run `make deploy`


## Improvements / TO-DOs

This is a quickmv  prototype and requires significant changes to be production ready.

- Rust
  - [ ] **Return Values from different stages**: We do not output anything , and all the user gets is a "Verification Successful". We need to refactor the `run_verification` function to return a result or error and propagate that upstream.
  - [ ] **Error Handling**: Non-existent, everything panics. 
  - [ ] **Run Asynchronously**: The job takes too long and times out. I have hacked the load balancer to increase time-outs, but ideally we should be returning a response immediately, and polling for a result.
  - [ ] Web 3 Provider : We should move this to the backend, and match the provider by network
  
- Infra
  - [ ] **DNS**: add a DNS, it's currently hard coded to the loadbalancer.
  - [ ] **Application Gateway**: Should be added for authentication, and rate limiting
  - [ ] **Private Service Discovery**: The front end communicates with zeth through the loadbalancer, which is public. The zeth service is necessarily exposed, and it should be done via a private endpoint.
- Front End
  - Absolutely nothing. I gave this my all, and refuse to spend any more time on it :grin:


