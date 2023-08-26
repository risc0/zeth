import { ApplicationLoadBalancer } from "@pulumi/awsx/lb";
import { Repository, Image } from "@pulumi/awsx/ecr";
import { Vpc } from "@pulumi/awsx/ec2";
import { SecurityGroup } from "@pulumi/aws/ec2";
import { Cluster } from "@pulumi/aws/ecs";
import { FargateService } from "@pulumi/awsx/ecs";
import * as apigateway from "@pulumi/aws-apigateway";


// Load Balancer
const lb = new ApplicationLoadBalancer("lb", {});

// Verification App : ECR Repository and Image
const app_repository = new Repository("app_repository", {});
const app_image = new Image("app", {
    repositoryUrl: app_repository.url,
    path: "../verification-app",
});


// Verification App : ECR Repository and Image
const zeth_repository = new Repository("zeth_repository", {});
const zeth_image = new Image("zeth", {
    repositoryUrl: zeth_repository.url,
    path: "../",
});


// VPC and Security Group
const vpc = new Vpc("vpc", {});
const securityGroup = new SecurityGroup("securityGroup", {
    vpcId: vpc.vpcId,
    egress: [{
        fromPort: 0,
        toPort: 0,
        protocol: "-1",
        cidrBlocks: ["0.0.0.0/0"],
        ipv6CidrBlocks: ["::/0"],
    }],
});

// ECS Cluster and Service
const cluster = new Cluster("cluster", {});// ECS Cluster and Service
const appService = new FargateService("service", {
    cluster: cluster.arn,
    desiredCount: 2,
    networkConfiguration: {
        subnets: vpc.publicSubnetIds,
        securityGroups: [securityGroup.id],
    },
    taskDefinitionArgs: {
        container: {
            name: "verification_app",
            image: app_image.imageUri,
            cpu: 512,
            memory: 128,
            essential: true,
            portMappings: [{
                containerPort: 3000,
                hostPort: 3000,
                protocol: "tcp",
            }],
        },
    },
});

// ECS Cluster and Service for Zeth
const zethService = new FargateService("zethService", {
    cluster: cluster.arn,
    desiredCount: 2,
    networkConfiguration: {
        subnets: vpc.publicSubnetIds,
        securityGroups: [securityGroup.id],
    },
    taskDefinitionArgs: {
        container: {
            name: "zeth",
            image: zeth_image.imageUri,
            cpu: 512,
            memory: 128,
            essential: true,
            portMappings: [{
                containerPort: 8000,
                hostPort: 8000,
                protocol: "tcp",
            }],
        },
    },
});

// Define an endpoint that proxies HTTP requests to the services.
const api = new apigateway.RestAPI("api", {
    routes: [
        {
            path: "/app",
            target: {
                type: "http_proxy",
                uri: lb.loadBalancer.dnsName.apply(dnsName => `http://${dnsName}:3000`),
            },
        },
        {
            path: "/zeth",
            target: {
                type: "http_proxy",
                uri: lb.loadBalancer.dnsName.apply(dnsName => `http://${dnsName}:8000`),
            },
        },
    ],
}, { dependsOn: lb });


export const loadbalancer_url = lb.loadBalancer;
export const clusterURN = cluster.urn;
export const appServiceName = appService.service.name;
export const zethServiceName = zethService.service.name;
export const securityGroupName = securityGroup.name;
export const imageUri = app_image.imageUri;
export const apiGateway = api.url;
