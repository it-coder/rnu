#!/bin/bash

# RNU (Rust Nexus Uploader) Usage Examples
# This script demonstrates various ways to use the nexus-upload CLI tool

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== RNU (Rust Nexus Uploader) Usage Examples ===${NC}\n"

# Check if nexus-upload is in PATH or use relative path
if command -v nexus-upload >/dev/null 2>&1; then
    NEXUS_UPLOAD="nexus-upload"
else
    NEXUS_UPLOAD="../target/release/nexus-upload"
fi

# Example 1: Initialize configuration
echo -e "${GREEN}Example 1: Initialize configuration${NC}"
echo "Command: $NEXUS_UPLOAD config init"
echo "Description: Creates a sample configuration file with default profiles"
echo

# Example 2: Show current configuration
echo -e "${GREEN}Example 2: Show current configuration${NC}"
echo "Command: $NEXUS_UPLOAD config show"
echo "Description: Displays the current configuration file content"
echo

# Example 3: List available profiles
echo -e "${GREEN}Example 3: List available profiles${NC}"
echo "Command: $NEXUS_UPLOAD config list"
echo "Description: Shows all configured profiles"
echo

# Example 4: Add a new profile
echo -e "${GREEN}Example 4: Add a new profile${NC}"
echo "Command: $NEXUS_UPLOAD config add staging --server https://staging-nexus.company.com --repository npm-hosted --user deploy"
echo "Description: Adds a new profile named 'staging'"
echo

# Example 5: Basic upload with command line arguments
echo -e "${GREEN}Example 5: Basic upload with CLI arguments${NC}"
echo "Command: $NEXUS_UPLOAD -s http://localhost:8081 -u admin -p password123 npm ./my-project"
echo "Description: Upload packages from './my-project/node_modules' to local Nexus"
echo

# Example 6: Upload using a profile
echo -e "${GREEN}Example 6: Upload using a profile${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project"
echo "Description: Use the 'local' profile configuration for server and user settings"
echo

# Example 7: Upload directly from node_modules
echo -e "${GREEN}Example 7: Upload directly from node_modules${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project/node_modules"
echo "Description: Directly specify the node_modules directory"
echo

# Example 8: Skip existing packages
echo -e "${GREEN}Example 8: Skip existing packages${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project --skip-existing"
echo "Description: Skip packages that already exist in the repository"
echo

# Example 9: Verbose output
echo -e "${GREEN}Example 9: Verbose output${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project --skip-existing -v"
echo "Description: Enable detailed logging to see what's happening"
echo

# Example 10: Dry run mode
echo -e "${GREEN}Example 10: Dry run mode${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project --dry-run"
echo "Description: Show what would be uploaded without actually uploading"
echo

# Example 11: Upload only specific packages
echo -e "${GREEN}Example 11: Upload only specific packages${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project --only \"lodash,express,react\""
echo "Description: Only upload packages containing 'lodash', 'express', or 'react'"
echo

# Example 12: Exclude specific packages
echo -e "${GREEN}Example 12: Exclude specific packages${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project --exclude \"test,mock,jest\""
echo "Description: Exclude packages containing 'test', 'mock', or 'jest' in their names"
echo

# Example 13: Complex filtering
echo -e "${GREEN}Example 13: Complex filtering${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project --skip-existing --only \"@types,lodash\" --exclude \"test\" -v"
echo "Description: Upload only @types and lodash packages, exclude test packages, skip existing ones, with verbose output"
echo

# Example 14: Different repository
echo -e "${GREEN}Example 14: Upload to different repository${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -r npm-snapshots -p password123 npm ./my-project"
echo "Description: Upload to 'npm-snapshots' repository instead of the default"
echo

# Example 15: Custom configuration file
echo -e "${GREEN}Example 15: Use custom configuration file${NC}"
echo "Command: $NEXUS_UPLOAD --config ./custom-config.json --profile production -p password123 npm ./my-project"
echo "Description: Use a custom configuration file instead of the default one"
echo

echo -e "${YELLOW}=== Common Use Cases ===${NC}\n"

echo -e "${BLUE}Use Case 1: Initial setup for new environment${NC}"
cat << 'EOF'
# 1. Initialize configuration
nexus-upload config init

# 2. Add your environment profile
nexus-upload config add myenv --server https://nexus.mycompany.com --repository npm-private --user myuser

# 3. Test with dry run
nexus-upload --profile myenv -p mypassword npm ./my-project --dry-run

# 4. Upload with skip existing
nexus-upload --profile myenv -p mypassword npm ./my-project --skip-existing
EOF
echo

echo -e "${BLUE}Use Case 2: Offline environment deployment${NC}"
cat << 'EOF'
# 1. Scan and show what would be uploaded
nexus-upload --profile production -p password npm ./offline-project --dry-run -v

# 2. Upload only production dependencies
nexus-upload --profile production -p password npm ./offline-project --exclude "test,dev,mock"

# 3. Upload remaining packages if needed
nexus-upload --profile production -p password npm ./offline-project --skip-existing
EOF
echo

echo -e "${BLUE}Use Case 3: Selective package upload${NC}"
cat << 'EOF'
# 1. Upload only framework packages
nexus-upload --profile local -p password npm ./project --only "react,vue,angular"

# 2. Upload only utility packages
nexus-upload --profile local -p password npm ./project --only "lodash,moment,axios"

# 3. Upload everything except development tools
nexus-upload --profile local -p password npm ./project --exclude "eslint,jest,webpack,babel"
EOF
echo

echo -e "${YELLOW}=== Security Best Practices ===${NC}\n"

echo "1. Never store passwords in configuration files"
echo "2. Use environment variables for passwords:"
echo "   export NEXUS_PASSWORD=your_password"
echo "   nexus-upload --profile local -p \$NEXUS_PASSWORD npm ./project"
echo
echo "3. Use secure password prompts in CI/CD:"
echo "   read -s NEXUS_PASSWORD"
echo "   nexus-upload --profile prod -p \$NEXUS_PASSWORD npm ./project"
echo

echo -e "${YELLOW}=== Troubleshooting ===${NC}\n"

echo "1. Connection issues:"
echo "   - Use -v flag for verbose output"
echo "   - Check server URL and network connectivity"
echo "   - Verify credentials"
echo
echo "2. Package not found errors:"
echo "   - Ensure node_modules contains the packages"
echo "   - Check that package.json files are present"
echo "   - Use --dry-run to see what packages are detected"
echo
echo "3. Upload failures:"
echo "   - Check repository permissions"
echo "   - Verify repository name is correct"
echo "   - Use --skip-existing to avoid duplicate errors"
echo

echo -e "${GREEN}For more information, run: $NEXUS_UPLOAD --help${NC}"
