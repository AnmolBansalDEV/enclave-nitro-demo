#!/bin/bash

# Stop SSM agent - Fix issue: https://github.com/amazonlinux/amazon-linux-2023/issues/397
systemctl stop amazon-ssm-agent

# Install Nitro CLI and enable necessary services
dnf install aws-nitro-enclaves-cli aws-nitro-enclaves-cli-devel -y
systemctl enable --now nitro-enclaves-allocator.service
systemctl enable --now docker

# Start SSM agent again
systemctl start amazon-ssm-agent

# Install ORAS CLI for pulling the enclave image
cd /root
VERSION="1.1.0"
curl -LO "https://github.com/oras-project/oras/releases/download/v${VERSION}/oras_${VERSION}_linux_amd64.tar.gz"
mkdir -p /root/oras-install/
tar -zxf oras_${VERSION}_*.tar.gz -C /root/oras-install/
mv /root/oras-install/oras /usr/local/bin/
rm -rf /root/oras_${VERSION}_*.tar.gz /root/oras-install/

# Install socat for forwarding
yum install socat -y

# Authenticate and pull the EIF artifact from Amazon ECR
aws ecr get-login-password --region us-east-2 | docker login --username AWS --password-stdin 717279690196.dkr.ecr.us-east-2.amazonaws.com
HOME=/root oras pull -o /root ${eifArtifactPath}

# Run socat to forward traffic
socat -t 30 TCP-LISTEN:80,fork,reuseaddr VSOCK-CONNECT:7777:1000 &

# Start the Nitro Enclave using the pulled EIF
nitro-cli run-enclave --cpu-count 2 --memory 512 --enclave-name test --enclave-cid 7777 --eif-path /root/enclave.eif --debug-mode
