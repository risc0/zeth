.PHONY: deploy setup

deploy:
	# Change directory into the infrastructure folder and run pulumi up
	cd infrastructure && pulumi up

setup:
	# Ensure Node.js 18 is installed
	if ! command -v node &> /dev/null; then \
		curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash -; \
		sudo apt-get install -y nodejs; \
	fi
	# Ensure Pulumi is installed
	if ! command -v pulumi &> /dev/null; then \
		curl -fsSL https://get.pulumi.com | sh; \
	fi
	# Ensure AWS CLI is installed
	if ! command -v aws &> /dev/null; then \
		curl "https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip" -o "awscliv2.zip"; \
		unzip awscliv2.zip; \
		sudo ./aws/install; \
	fi
	# Check AWS configuration
	if [ -z "$$AWS_ACCESS_KEY_ID" ] || [ -z "$$AWS_SECRET_ACCESS_KEY" ] || [ -z "$$AWS_DEFAULT_REGION" ]; then \
		read -p "Enter AWS Access Key ID: " access_key; \
		read -p "Enter AWS Secret Access Key: " secret_key; \
		read -p "Enter AWS Default Region: " region; \
		export AWS_ACCESS_KEY_ID=$$access_key; \
		export AWS_SECRET_ACCESS_KEY=$$secret_key; \
		export AWS_DEFAULT_REGION=$$region; \
	fi
	# Prompt for S3 bucket name
	read -p "Enter S3 bucket name: " bucket_name; \
	# Create S3 bucket for Pulumi backend
	aws s3api create-bucket --bucket $$bucket_name --region $$AWS_DEFAULT_REGION
	# Login to Pulumi
	pulumi login s3://$$bucket_name
	# Prompt for Pulumi config passphrase
	read -sp "Enter Pulumi config passphrase: " passphrase; \
	export PULUMI_CONFIG_PASSPHRASE=$$passphrase