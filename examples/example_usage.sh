#!/bin/sh

# RNU (Rust Nexus Uploader) usage examples.
#
# 本脚本随发布包一起分发，因此刻意只用 POSIX sh 语法：
# echo -e 不是 POSIX（dash 会把 -e 当字面文本打印出来），统一改用 printf '%b'。

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# printf '%b' 负责解释上面的 \033 转义，等价于 echo -e 但可移植
say() { printf '%b\n' "$1"; }

say "${BLUE}=== RNU (Rust Nexus Uploader) Usage Examples ===${NC}\n"

# 定位二进制：优先 PATH，其次仓库构建产物，最后发布包内与本脚本同级的文件。
BIN=nexus-upload
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*) BIN=nexus-upload.exe ;;
esac

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

if command -v "$BIN" >/dev/null 2>&1; then
    NEXUS_UPLOAD="$BIN"
elif [ -x "$SCRIPT_DIR/../target/release/$BIN" ]; then
    NEXUS_UPLOAD="$SCRIPT_DIR/../target/release/$BIN"
elif [ -x "$SCRIPT_DIR/$BIN" ]; then
    NEXUS_UPLOAD="$SCRIPT_DIR/$BIN"
else
    NEXUS_UPLOAD="$BIN"
fi

# ── Configuration Management ───────────────────────────────────────

say "${GREEN}Example 1: Initialize configuration${NC}"
echo "Command: $NEXUS_UPLOAD config init"
echo "Description: Creates a sample configuration file with default profiles"
echo

say "${GREEN}Example 2: Show current configuration${NC}"
echo "Command: $NEXUS_UPLOAD config show"
echo "Description: Displays the current configuration file content"
echo

say "${GREEN}Example 3: List available profiles${NC}"
echo "Command: $NEXUS_UPLOAD config list"
echo "Description: Shows all configured profiles"
echo

say "${GREEN}Example 4: Add a new profile${NC}"
echo "Command: $NEXUS_UPLOAD config add staging --server https://staging-nexus.company.com --repository npm-hosted --user deploy"
echo "Description: Adds a new profile named 'staging'"
echo

# ── npm Upload ─────────────────────────────────────────────────────

say "${YELLOW}=== npm Upload Examples ===${NC}\n"

say "${GREEN}Example 5: Basic npm upload${NC}"
echo "Command: $NEXUS_UPLOAD -s http://localhost:8081 -u admin -p password123 npm ./my-project"
echo "Description: Upload packages from './my-project/node_modules' to local Nexus"
echo

say "${GREEN}Example 6: npm upload with profile${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project"
echo "Description: Use the 'local' profile for server and user settings"
echo

say "${GREEN}Example 7: npm — skip existing + concurrency${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project --skip-existing -c 8"
echo "Description: Skip existing packages, upload with 8 concurrent workers"
echo

say "${GREEN}Example 8: npm — verbose mode${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project --skip-existing -v"
echo "Description: Enable INFO-level tracing to see scan / upload progress"
echo

say "${GREEN}Example 9: npm — dry-run${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project --dry-run"
echo "Description: Preview what would be uploaded without actually uploading"
echo

say "${GREEN}Example 10: npm — only specific packages${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project --only \"lodash,express,react\""
echo "Description: Only upload packages matching lodash, express, or react"
echo

say "${GREEN}Example 11: npm — exclude packages${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project --exclude \"test,mock,jest\""
echo "Description: Exclude test/mock/jest packages"
echo

say "${GREEN}Example 12: npm — complex filtering${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 npm ./my-project --skip-existing --only \"@types,lodash\" --exclude \"test\" -c 4 -v"
echo "Description: Upload @types and lodash, exclude test, skip existing, 4 workers, verbose"
echo

# ── Maven Upload ───────────────────────────────────────────────────

say "${YELLOW}=== Maven Upload Examples ===${NC}\n"

say "${GREEN}Example 13: Maven — upload target directory${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 -r maven-releases maven ./target"
echo "Description: Scan ./target for .pom + .jar files and upload to maven-releases"
echo

say "${GREEN}Example 14: Maven — from local repository${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 maven ~/.m2/repository --skip-existing -c 4"
echo "Description: Scan local Maven repository, skip existing artifacts, 4 concurrent uploads"
echo

say "${GREEN}Example 15: Maven — filter by artifactId${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 -r maven-releases maven ./target --only \"my-app,my-lib\""
echo "Description: Only upload artifacts whose artifactId contains my-app or my-lib"
echo

say "${GREEN}Example 16: Maven — dry-run preview${NC}"
echo "Command: $NEXUS_UPLOAD --profile local -p password123 maven ./target --dry-run -v"
echo "Description: Scan and show what Maven artifacts would be uploaded"
echo

# ── Common Use Cases ───────────────────────────────────────────────

say "${YELLOW}=== Common Use Cases ===${NC}\n"

say "${BLUE}Use Case 1: Initial setup for new environment${NC}"
cat << 'EOF'
# 1. Initialize configuration
nexus-upload config init

# 2. Add your environment profile
nexus-upload config add myenv --server https://nexus.mycompany.com --repository npm-private --user myuser

# 3. Test with dry run (npm)
nexus-upload --profile myenv -p mypassword npm ./my-project --dry-run

# 4. Upload with skip existing + concurrency
nexus-upload --profile myenv -p mypassword npm ./my-project --skip-existing -c 8
EOF
echo

say "${BLUE}Use Case 2: Offline environment deployment (npm)${NC}"
cat << 'EOF'
# 1. Scan and show what would be uploaded
nexus-upload --profile production -p password npm ./offline-project --dry-run -v

# 2. Upload production dependencies only
nexus-upload --profile production -p password npm ./offline-project --exclude "test,dev,mock" -c 8

# 3. Upload remaining packages
nexus-upload --profile production -p password npm ./offline-project --skip-existing
EOF
echo

say "${BLUE}Use Case 3: Maven build output upload${NC}"
cat << 'EOF'
# 1. Build your project
mvn clean package

# 2. Preview upload
nexus-upload --profile nexus -p $PASSWORD -r maven-releases maven ./target --dry-run

# 3. Upload with concurrency
nexus-upload --profile nexus -p $PASSWORD -r maven-releases maven ./target -c 8

# 4. Upload snapshots
nexus-upload --profile nexus -p $PASSWORD -r maven-snapshots maven ./target --skip-existing
EOF
echo

# ── Security Best Practices ────────────────────────────────────────

say "${YELLOW}=== Security Best Practices ===${NC}\n"

echo "1. Never store passwords in configuration files"
echo "2. Use environment variables for passwords:"
echo "   export NEXUS_PASSWORD=your_password"
echo "   nexus-upload --profile local -p \$NEXUS_PASSWORD npm ./project"
echo
echo "3. Use secure password prompts in CI/CD:"
echo "   read -s NEXUS_PASSWORD"
echo "   nexus-upload --profile prod -p \$NEXUS_PASSWORD npm ./project"
echo

# ── Troubleshooting ────────────────────────────────────────────────

say "${YELLOW}=== Troubleshooting ===${NC}\n"

echo "1. Connection issues:"
echo "   - Use -v flag for verbose output (INFO level)"
echo "   - Use RUST_LOG=debug for detailed diagnostics"
echo "   - Check server URL and network connectivity"
echo
echo "2. Package not found errors:"
echo "   - npm: ensure node_modules contains package.json files"
echo "   - Maven: ensure target/ contains .pom and .jar files"
echo "   - Use --dry-run to see what is detected"
echo
echo "3. Upload failures:"
echo "   - Check repository permissions (401 = auth, 403 = permission)"
echo "   - Verify repository name matches Nexus configuration"
echo "   - Use --skip-existing to avoid 409 conflicts"
echo "   - Reduce concurrency with -c 1 to isolate issues"
echo

say "${GREEN}For more information, run: $NEXUS_UPLOAD --help${NC}"
