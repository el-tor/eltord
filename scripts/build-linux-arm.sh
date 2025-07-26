#!/bin/bash

# Build script for Linux ARM64 using act (Docker-based)
# This runs the GitHub Actions workflow locally via act

# Ensure we're in the correct directory
cd "$(dirname "$0")/.."

# Run act with proper volume binding to ensure artifacts persist
# Skip self-hosted runners by using a platform that doesn't exist
ACT=true act workflow_dispatch --secret-file .secrets -j build-linux-arm -P self-hosted=skip --bind