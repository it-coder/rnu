.PHONY: build release test clean install uninstall help run-tests format lint check-deps example

# Default target
all: build

# Build in debug mode
build:
	cargo build

# Build in release mode
release:
	cargo build --release

# Run all tests
test:
	cargo test

# Run tests with output
test-verbose:
	cargo test -- --nocapture

# Clean build artifacts
clean:
	cargo clean
	rm -rf target/

# Install the binary to system PATH
install: release
	sudo cp target/release/nexus-upload /usr/local/bin/
	chmod +x /usr/local/bin/nexus-upload
	@echo "nexus-upload installed to /usr/local/bin/"

# Install to user's local bin (no sudo required)
install-user: release
	mkdir -p ~/.local/bin
	cp target/release/nexus-upload ~/.local/bin/
	chmod +x ~/.local/bin/nexus-upload
	@echo "nexus-upload installed to ~/.local/bin/"
	@echo "Make sure ~/.local/bin is in your PATH"

# Uninstall from system
uninstall:
	sudo rm -f /usr/local/bin/nexus-upload
	@echo "nexus-upload uninstalled from /usr/local/bin/"

# Uninstall from user local
uninstall-user:
	rm -f ~/.local/bin/nexus-upload
	@echo "nexus-upload uninstalled from ~/.local/bin/"

# Check code formatting
format:
	cargo fmt

# Check if code is properly formatted
format-check:
	cargo fmt -- --check

# Run clippy lints
lint:
	cargo clippy -- -D warnings

# Run all checks (format, lint, test)
check: format-check lint test

# Check dependencies for security vulnerabilities
check-deps:
	cargo audit

# Show dependencies
deps:
	cargo tree

# Update dependencies
update:
	cargo update

# Generate documentation
docs:
	cargo doc --open

# Run examples
example:
	@echo "Running example commands..."
	./examples/example_usage.sh

# Package for distribution
package: release
	mkdir -p dist
	cp target/release/nexus-upload dist/
	cp README.md dist/
	cp config.example.json dist/
	cp examples/example_usage.sh dist/
	tar -czf dist/rnu-$(shell cargo pkgid | cut -d# -f2).tar.gz -C dist .
	@echo "Package created in dist/"

# Run development version with example args
dev-run:
	cargo run -- --help

# Watch for changes and rebuild
watch:
	cargo watch -x build

# Initialize configuration for testing
init-config:
	./target/release/nexus-upload config init

# Show configuration
show-config:
	./target/release/nexus-upload config show

# List profiles
list-profiles:
	./target/release/nexus-upload config list

# Benchmark build times
bench-build:
	@echo "Benchmarking build times..."
	time cargo build
	cargo clean
	time cargo build --release

# Create a new release tag
tag:
	@echo "Current version: $(shell cargo pkgid | cut -d# -f2)"
	@read -p "Enter new version: " version; \
	sed -i.bak "s/^version = .*/version = \"$$version\"/" Cargo.toml; \
	git add Cargo.toml; \
	git commit -m "Bump version to $$version"; \
	git tag -a "v$$version" -m "Release version $$version"

# Setup development environment
setup-dev:
	@echo "Setting up development environment..."
	rustup update
	rustup component add clippy rustfmt
	cargo install cargo-watch cargo-audit
	@echo "Development setup complete!"

# Quick test with dry run
test-dry-run:
	@echo "Testing dry run mode..."
	./target/release/nexus-upload --profile local -p dummy npm . --dry-run || echo "This is expected to fail without proper config"

# Performance profile
profile:
	cargo build --release
	@echo "Binary size:"
	ls -lh target/release/nexus-upload
	@echo "Dependencies:"
	ldd target/release/nexus-upload 2>/dev/null || otool -L target/release/nexus-upload 2>/dev/null || echo "Cannot show dependencies on this system"

# Help target
help:
	@echo "Available targets:"
	@echo "  build          - Build in debug mode"
	@echo "  release        - Build in release mode"
	@echo "  test           - Run tests"
	@echo "  test-verbose   - Run tests with output"
	@echo "  clean          - Clean build artifacts"
	@echo "  install        - Install to /usr/local/bin (requires sudo)"
	@echo "  install-user   - Install to ~/.local/bin"
	@echo "  uninstall      - Remove from /usr/local/bin"
	@echo "  uninstall-user - Remove from ~/.local/bin"
	@echo "  format         - Format code"
	@echo "  format-check   - Check code formatting"
	@echo "  lint           - Run clippy lints"
	@echo "  check          - Run all checks (format, lint, test)"
	@echo "  check-deps     - Check dependencies for vulnerabilities"
	@echo "  deps           - Show dependency tree"
	@echo "  update         - Update dependencies"
	@echo "  docs           - Generate and open documentation"
	@echo "  example        - Run usage examples"
	@echo "  package        - Create distribution package"
	@echo "  dev-run        - Run development version"
	@echo "  watch          - Watch for changes and rebuild"
	@echo "  init-config    - Initialize configuration"
	@echo "  show-config    - Show current configuration"
	@echo "  list-profiles  - List available profiles"
	@echo "  bench-build    - Benchmark build times"
	@echo "  tag            - Create new version tag"
	@echo "  setup-dev      - Setup development environment"
	@echo "  test-dry-run   - Test dry run functionality"
	@echo "  profile        - Show binary info and dependencies"
	@echo "  help           - Show this help message"
