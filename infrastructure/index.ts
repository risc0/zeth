import * as awsx from "@pulumi/awsx";

import * as aws from "@pulumi/aws";

// Create a new log group for appService
const appServiceLogGroup = new aws.cloudwatch.LogGroup("appServiceLogGroup", {
    name: "/ecs/appService",
});

// Create a new log group for zethService
const zethServiceLogGroup = new aws.cloudwatch.LogGroup("zethServiceLogGroup", {
    name: "/ecs/zethService",
});

// Create an ECS Fargate cluster.
const cluster = new awsx.classic.ecs.Cluster("cluster");

// Define the Networking for our service.
const alb = new awsx.classic.lb.ApplicationLoadBalancer(
    "net-lb", { external: true, securityGroups: cluster.securityGroups });
const web = alb.createListener("web", { port: 3000, external: true, protocol: "HTTP" });
const zeth_listener = alb.createListener("zeth", { port: 8000, external: true, protocol: "HTTP" });

// Verification App : ECR Repository and Image
const app_repository = new awsx.ecr.Repository("app_repository", {
    forceDelete: true,
});
const app_image = new awsx.ecr.Image("app", {
    repositoryUrl: app_repository.url,
    path: "/home/ubuntu/zeth/verification-app/",
    dockerfile: "/home/ubuntu/zeth/verification-app/Dockerfile"

});


// Zeth : ECR Repository and Image
const zeth_repository = new awsx.ecr.Repository("zeth_repository", {
    forceDelete: true,
});
const zeth_image = new awsx.ecr.Image("zeth", {
    repositoryUrl: zeth_repository.url,
    path: "/home/ubuntu/zeth/zeth/",
    dockerfile: "/home/ubuntu/zeth/zeth/Dockerfile"
});

const appService = new awsx.classic.ecs.FargateService("appService", {
    cluster,
    desiredCount: 2,
    taskDefinitionArgs: {
        container: {
            image: app_image.imageUri,
            cpu: 512,
            memory: 128,
            essential: true,
            portMappings: [web],
            logConfiguration: {
                logDriver: "awslogs",
                options: {
                    "awslogs-group": appServiceLogGroup.name,
                    "awslogs-region": "us-east-1",
                    "awslogs-stream-prefix": "ecs"
                }
            },
        },
    },
}, { dependsOn: alb });

const zethService = new awsx.classic.ecs.FargateService("zethService", {
    cluster,
    desiredCount: 2,
    taskDefinitionArgs: {
        container: {
            image: zeth_image.imageUri,
            cpu: 4096,
            memory: 4096,
            essential: true,
            portMappings: [zeth_listener],
            logConfiguration: {
                logDriver: "awslogs",
                options: {
                    "awslogs-group" :zethServiceLogGroup.name,
                    "awslogs-region": "us-east-1",
                    "awslogs-stream-prefix": "ecs"
                }
            },
        }
    },
}, { dependsOn: alb });



export const clusterURN = cluster.urn;
export const appServiceName = appService.service.name;
export const zethServiceName = zethService.service.name;
export const appImageUri = app_image.imageUri;
export const zethImageUri = zeth_image.imageUri;
export const appUrl = web.endpoint.hostname;
export const zethUrl = zeth_listener.endpoint.hostname;