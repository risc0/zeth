import * as awsx from "@pulumi/awsx";
import * as aws from "@pulumi/aws";
import * as AWS from "aws-sdk";
import * as pulumi from "@pulumi/pulumi";
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
// Create a target group for the web listener on port 3000
// const webTg = new awsx.lb.TargetGroup("webTg", { port: 3000, protocol: "HTTP" });
// Create a target group on port 3000
const webTg = new aws.lb.TargetGroup("webTg", {
    port: 3000,
    protocol: "HTTP",
    targetType: "ip",
    vpcId: alb.vpc.id,
});

// Create a listener rule to forward traffic to the target group
const webListenerRule = new aws.lb.ListenerRule("webListenerRule", {
    actions: [{
        type: "forward",
        targetGroupArn: webTg.arn,
    }],
    conditions: [{
        pathPattern: {
            values: ["/*"], // replace with your path pattern
        },
    }],
    listenerArn: web.listener.arn,
    priority: 100,
});
// Create a target group for the web listener
// const webTg = new awsx.lb.TargetGroup("webTg", { port: 3000, protocol: "HTTP" });
// web.listener.setDefaultAction(webTg);
const zethListener = alb.createListener("zethListener", { port: 8000, external: true, protocol: "HTTP" });



// Verification App : ECR Repository and Image
const app_repository = new awsx.ecr.Repository("app_repository", {
    forceDelete: true,
});
const app_image = new awsx.ecr.Image("app", {
    repositoryUrl: app_repository.url,
    path: "../verification-app/",
    dockerfile: "../verification-app/Dockerfile"

});


// Zeth : ECR Repository and Image
const zeth_repository = new awsx.ecr.Repository("zeth_repository", {
    forceDelete: true,
});
const zeth_image = new awsx.ecr.Image("zeth", {
    repositoryUrl: zeth_repository.url,
    path: "../zeth/",
    dockerfile: "../zeth/Dockerfile"
});


const appService =
    new awsx.classic.ecs.FargateService("appService", {
        cluster,
        desiredCount: 2,
        taskDefinitionArgs: {
            cpu: "4096",
            memory: "8192",
            containers: {
                appServiceContainer: {
                    image: app_image.imageUri,
                    cpu: 512,
                    memory: 512,
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
                zethServiceContainer: {
                    image: zeth_image.imageUri,
                    cpu: 1024,
                    memory: 1024,
                    essential: true,
                    portMappings: [zethListener],
                    logConfiguration: {
                        logDriver: "awslogs",
                        options: {
                            "awslogs-group": zethServiceLogGroup.name,
                            "awslogs-region": "us-east-1",
                            "awslogs-stream-prefix": "ecs"
                        }
                    },
                }
            },
        },
    }, { dependsOn: alb });



const getTaskIpAddresses = async () => {
    const ecs = new AWS.ECS();
    const tasks = await ecs.listTasks({ cluster: cluster.id.get(), serviceName: appService.service.name.get() }).promise();
    if (!tasks.taskArns) {
        throw new Error("No tasks found");
    }
    const taskDetails = await ecs.describeTasks({ cluster: cluster.id.get(), tasks: tasks.taskArns }).promise();
    if (!taskDetails.tasks) {
        throw new Error("No task details found");
    }
    return taskDetails.tasks.map(task => {
        if (task.attachments && task.attachments[0] && task.attachments[0].details) {
            return (task.attachments[0].details.find(detail => detail.name === "networkInterfaceId") || {}).value || null;
        }
        return null; // or some default value
    }).filter(value => value !== null); // filter out null values
};

// Register the appService as a target in the webTg target group
// const webTgAttachment = new aws.lb.TargetGroupAttachment("webTgAttachment", {
//     targetGroupArn: webTg.arn,
//     targetId: appService.service.id,
//     port: 3000,
// });

// Register the appService as a target in the webTg target group
const webTgAttachment = new aws.lb.TargetGroupAttachment("webTgAttachment", {
    targetGroupArn: webTg.arn,
    targetId: pulumi.output(getTaskIpAddresses()).apply(ipAddresses => {
        const validIpAddresses = ipAddresses.filter(ip => ip !== null);
        if (validIpAddresses.length === 0) {
            return "default-value"; // replace "default-value" with a suitable default
        }
        return validIpAddresses[0];
    }), 
    port: 3000,
});

export const clusterURN = cluster.urn;
// export const appServiceName = appService.service.name;
export const appImageUri = app_image.imageUri;
export const zethImageUri = zeth_image.imageUri;
export const appUrl = web.endpoint.hostname;
export const zethUrl = zethListener.endpoint.hostname;


