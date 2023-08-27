#!/bin/bash

# Replace with your actual load balancer ARN
load_balancer_arn="arn:aws:elasticloadbalancing:us-east-1:803035318642:loadbalancer/app/lb-7af9865/f3804e4492cdbfc8"

# Get all target groups and their associated load balancers
target_groups=$(aws elbv2 describe-target-groups --query 'TargetGroups[*].[TargetGroupArn, LoadBalancerArns[]]' --output text)
echo "Target groups and their associated load balancers:"
echo "$target_groups"

# Find the security groups associated with the load balancer
security_groups=$(aws elbv2 describe-load-balancers --load-balancer-arns $load_balancer_arn --query "LoadBalancers[*].SecurityGroups[*]" --output text)
echo "Security groups associated with the load balancer:"
echo $security_groups


aws elb list-dependency-issues arn:aws:elasticloadbalancing:us-east-1:803035318642:loadbalancer/app/lb-7af9865/f3804e4492cdbfc8