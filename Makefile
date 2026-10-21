# rnu — 构建 / 测试 / 打包
#
# 平台支持：macOS、Linux、Windows（Windows 需要 MSYS2 或 Git Bash 提供
# make / sh / coreutils —— cmd.exe 与 PowerShell 无法直接执行这些 recipe）。
#
# 交叉打包：给 package 传 TARGET 即可
#     make package                              # 宿主平台
#     make package TARGET=x86_64-apple-darwin   # 指定其它平台
#     make targets                              # 一次性安装常用 target
#     make package-all                          # 按 PLATFORMS 逐个打（需交叉工具链）
# 注意：交叉 target 能否链接成功取决于本机工具链，本机打不出来时请改用 CI。

# ── 可覆盖变量 ──────────────────────────────────────────────────────
BIN       ?= nexus-upload
CARGO     ?= cargo
RUSTUP    ?= rustup

# 目标平台 triple；留空表示构建宿主平台
TARGET    ?=

# package-all 使用的平台清单。默认偏向“能从 macOS 交叉出去”的组合；
# Windows 上用 msvc 更原生，可在命令行覆盖：make package-all PLATFORMS=x86_64-pc-windows-msvc
PLATFORMS ?= aarch64-apple-darwin x86_64-apple-darwin \
             x86_64-unknown-linux-musl aarch64-unknown-linux-musl \
             x86_64-pc-windows-gnu

DIST      ?= dist

# 新版本号；给 tag 传值即可跳过交互提问：make tag NEW_VERSION=0.2.0
NEW_VERSION ?=

# ── 派生变量 ────────────────────────────────────────────────────────

# 宿主 triple —— TARGET 为空时用于给产物命名
HOST      := $(shell $(RUSTUP) show 2>/dev/null | sed -n 's/^Default host: //p')
ifeq ($(strip $(HOST)),)
HOST      := $(shell rustc -vV 2>/dev/null | sed -n 's/^host: //p')
endif

TRIPLE    := $(if $(TARGET),$(TARGET),$(HOST))

# 指定 TARGET 时 cargo 会多套一层 target/<triple>/
RELEASE_DIR := target/$(if $(TARGET),$(TARGET)/,)release
DEBUG_DIR   := target/$(if $(TARGET),$(TARGET)/,)debug

# Windows 产物带 .exe —— 按 triple 判定而不是按宿主 OS，交叉编译时这点是关键
EXE       := $(if $(findstring windows,$(TRIPLE)),.exe,)

TARGET_FLAG := $(if $(TARGET),--target $(TARGET),)

# 版本号直接读 Cargo.toml，不依赖 cargo pkgid 的输出格式
VERSION   := $(shell sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)

PKG       := rnu-$(VERSION)-$(TRIPLE)

# 校验和工具探测：GNU 系用 sha256sum，macOS 用 shasum -a 256，
# Git Bash 这类两者都缺的环境退回 openssl（输出仍是 hash + 文件名，可被 -c 校验）
SHA256    := $(shell command -v sha256sum >/dev/null 2>&1 && echo "sha256sum" \
	            || (command -v shasum >/dev/null 2>&1 && echo "shasum -a 256" \
	            || (command -v openssl >/dev/null 2>&1 && echo "openssl dgst -sha256 -r")))

.PHONY: all build release test test-verbose run-tests clean install install-user \
        uninstall uninstall-user format format-check lint check check-deps deps update \
        docs example package package-all targets dev-run watch init-config show-config \
        list-profiles bench-build tag setup-dev test-dry-run profile help

# ── 默认目标 ────────────────────────────────────────────────────────
all: build

# ── 构建 ────────────────────────────────────────────────────────────

build:
	$(CARGO) build $(TARGET_FLAG)

release:
	$(CARGO) build --release $(TARGET_FLAG)

clean:
	$(CARGO) clean
	rm -rf $(DIST)

# ── 测试与检查 ──────────────────────────────────────────────────────

test:
	$(CARGO) test

test-verbose:
	$(CARGO) test -- --nocapture

# 旧名保留为别名
run-tests: test

format:
	$(CARGO) fmt

format-check:
	$(CARGO) fmt -- --check

lint:
	$(CARGO) clippy -- -D warnings

check: format-check lint test

check-deps:
	$(CARGO) audit

# ── 依赖 ────────────────────────────────────────────────────────────

deps:
	$(CARGO) tree

update:
	$(CARGO) update

# ── 文档 ────────────────────────────────────────────────────────────

docs:
	$(CARGO) doc --no-deps --open

# ── 安装（Unix 专属：依赖 sudo / chmod 与 FHS 目录）─────────────────

ifeq ($(OS),Windows_NT)
install install-user uninstall uninstall-user:
	@echo "install / install-user / uninstall 系列仅支持 macOS 与 Linux。"
	@echo "Windows 上请手动把 target/release/$(BIN).exe 复制到 PATH 中的任意目录。"
	@exit 1
else
install: release
	sudo cp $(RELEASE_DIR)/$(BIN)$(EXE) /usr/local/bin/
	chmod +x /usr/local/bin/$(BIN)
	@echo "$(BIN) installed to /usr/local/bin/"

install-user: release
	mkdir -p ~/.local/bin
	cp $(RELEASE_DIR)/$(BIN)$(EXE) ~/.local/bin/
	chmod +x ~/.local/bin/$(BIN)
	@echo "$(BIN) installed to ~/.local/bin/"
	@echo "Make sure ~/.local/bin is in your PATH"

uninstall:
	sudo rm -f /usr/local/bin/$(BIN)
	@echo "$(BIN) uninstalled from /usr/local/bin/"

uninstall-user:
	rm -f ~/.local/bin/$(BIN)
	@echo "$(BIN) uninstalled from ~/.local/bin/"
endif

# ── 打包 ────────────────────────────────────────────────────────────

# 产物落在 dist/ 下，tar 内是单层目录，解包不会散落一地：
#     dist/rnu-0.1.0-aarch64-apple-darwin/
#         nexus-upload  readme.md  config.example.json  example_usage.sh
#     dist/rnu-0.1.0-aarch64-apple-darwin.tar.gz
#     dist/rnu-0.1.0-aarch64-apple-darwin.tar.gz.sha256
package: release
	@echo "Packaging $(BIN) for $(TRIPLE)"
	@rm -rf $(DIST)/$(PKG)
	@mkdir -p $(DIST)/$(PKG)
	cp $(RELEASE_DIR)/$(BIN)$(EXE) $(DIST)/$(PKG)/
	cp readme.md $(DIST)/$(PKG)/
	cp config.example.json $(DIST)/$(PKG)/
	cp examples/example_usage.sh $(DIST)/$(PKG)/
	@cd $(DIST) && tar -czf $(PKG).tar.gz $(PKG)
ifneq ($(strip $(SHA256)),)
	@cd $(DIST) && $(SHA256) $(PKG).tar.gz > $(PKG).tar.gz.sha256
endif
	@ls -lh $(DIST)/$(PKG).tar.gz
	@echo "✓ $(DIST)/$(PKG).tar.gz"

# 一次性安装常用 target
targets:
	$(RUSTUP) target add \
	  aarch64-apple-darwin x86_64-apple-darwin \
	  x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
	  x86_64-unknown-linux-musl aarch64-unknown-linux-musl \
	  x86_64-pc-windows-msvc x86_64-pc-windows-gnu

# 逐个平台打包。缺工具链的目标会明确报错并继续，最后给出汇总与补齐提示。
package-all:
	@echo "Platforms: $(PLATFORMS)"
	@ok=""; failed=""; \
	for t in $(PLATFORMS); do \
	  printf '\n── %s\n' "$$t"; \
	  if $(MAKE) --no-print-directory package TARGET=$$t; then \
	    ok="$$ok $$t"; \
	  else \
	    failed="$$failed $$t"; \
	  fi; \
	done; \
	printf '\n汇总\n  成功:%s\n' "$${ok:- 无}"; \
	if [ -n "$$failed" ]; then \
	  printf '  失败:%s\n\n' "$$failed"; \
	  printf '失败目标的工具链要求:\n'; \
	  printf '  *-apple-darwin        rustup target add 即可，无需额外链接器\n'; \
	  printf '  x86_64-pc-windows-gnu 需要 mingw-w64（macOS: brew install mingw-w64）\n'; \
	  printf '  x86_64-pc-windows-msvc 只能在 Windows 上构建\n'; \
	  printf '  *-unknown-linux-*     需要 cross + Docker，或 cargo-zigbuild + zig\n'; \
	  printf '\n本机打不出的平台建议交给 CI（各平台原生 runner 各自构建）。\n'; \
	  exit 1; \
	fi

# ── 开发辅助 ────────────────────────────────────────────────────────

example:
	sh examples/example_usage.sh

dev-run:
	$(CARGO) run -- --help

watch:
	$(CARGO) watch -x build

init-config: release
	./$(RELEASE_DIR)/$(BIN)$(EXE) config init

show-config: release
	./$(RELEASE_DIR)/$(BIN)$(EXE) config show

list-profiles: release
	./$(RELEASE_DIR)/$(BIN)$(EXE) config list

test-dry-run: release
	@echo "Testing dry run mode..."
	@./$(RELEASE_DIR)/$(BIN)$(EXE) --profile local -p dummy npm . --dry-run || echo "This is expected to fail without proper config"

# 冷/热构建耗时对照。冷构建隔离到 target-bench，不再清空 target/。
bench-build:
	@echo "Cold build (isolated target-bench, target/ 不受影响):"
	@rm -rf target-bench
	@sh -c 'time CARGO_TARGET_DIR=target-bench $(CARGO) build'
	@rm -rf target-bench
	@echo "Warm build:"
	@sh -c 'time $(CARGO) build'

# 二进制体积与动态依赖
profile: release
	@echo "Binary:"
	@ls -lh $(RELEASE_DIR)/$(BIN)$(EXE)
	@echo "Dependencies:"
	@file $(RELEASE_DIR)/$(BIN)$(EXE) 2>/dev/null || true
	@ldd $(RELEASE_DIR)/$(BIN)$(EXE) 2>/dev/null \
	  || otool -L $(RELEASE_DIR)/$(BIN)$(EXE) 2>/dev/null \
	  || echo "  (Windows 或静态链接：无动态依赖信息)"

# 发版：同步 Cargo.toml 与 src/cli.rs 的版本号（clap 的 version 是硬编码的，
# 漏改会导致 nexus-upload --version 停留在旧版本），然后提交并打 tag。
#     make tag NEW_VERSION=0.2.0
#     make tag                    # 不带参数则交互式询问
tag:
	@v="$(NEW_VERSION)"; \
	if [ -z "$$v" ]; then printf "Enter new version: "; read v; fi; \
	echo "$$v" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.+-]+)?$$' \
	  || { echo "非法版本号: '$$v'（应形如 0.2.0）"; exit 1; }; \
	echo "Bumping $(VERSION) -> $$v"; \
	sed "s/^version[[:space:]]*=[[:space:]]*\"[^\"]*\"/version = \"$$v\"/" Cargo.toml > Cargo.toml.tmp \
	  && mv Cargo.toml.tmp Cargo.toml || exit 1; \
	$(CARGO) check -q || exit 1; \
	git add Cargo.toml Cargo.lock; \
	git commit -m "chore: bump version to $$v"; \
	git tag -a "v$$v" -m "Release v$$v"; \
	echo "✓ committed and tagged v$$v"

setup-dev:
	@echo "Setting up development environment..."
	$(RUSTUP) update
	$(RUSTUP) component add clippy rustfmt
	$(CARGO) install cargo-watch cargo-audit
	@echo "Optional, for cross-platform packaging:"
	@echo "  $(CARGO) install cross          # 走 Docker，可打 Linux 包"
	@echo "  $(CARGO) install cargo-zigbuild # 配合 zig，可打 Linux/Windows 包"
	@echo "Development setup complete!"

# ── 帮助 ────────────────────────────────────────────────────────────
help:
	@echo "Available targets:"
	@echo "  build            - 调试构建（可加 TARGET=<triple>）"
	@echo "  release          - 发布构建（可加 TARGET=<triple>）"
	@echo "  test             - 运行测试"
	@echo "  test-verbose     - 带输出运行测试"
	@echo "  clean            - 清理 target/ 与 dist/"
	@echo "  package          - 打包到 dist/（可加 TARGET=<triple>）"
	@echo "  package-all      - 按 PLATFORMS 逐个打包"
	@echo "  targets          - rustup target add 常用平台"
	@echo "  install          - 安装到 /usr/local/bin（需 sudo，仅 Unix）"
	@echo "  install-user     - 安装到 ~/.local/bin（仅 Unix）"
	@echo "  uninstall        - 从 /usr/local/bin 移除（仅 Unix）"
	@echo "  uninstall-user   - 从 ~/.local/bin 移除（仅 Unix）"
	@echo "  format           - cargo fmt"
	@echo "  format-check     - 检查格式"
	@echo "  lint             - cargo clippy -- -D warnings"
	@echo "  check            - format-check + lint + test"
	@echo "  check-deps       - cargo audit"
	@echo "  deps             - cargo tree"
	@echo "  update           - cargo update"
	@echo "  docs             - 生成并打开文档"
	@echo "  example          - 运行使用示例脚本"
	@echo "  dev-run          - cargo run -- --help"
	@echo "  watch            - cargo watch -x build"
	@echo "  init-config      - 初始化配置文件"
	@echo "  show-config      - 显示当前配置"
	@echo "  list-profiles    - 列出所有 profile"
	@echo "  test-dry-run     - 干运行自检"
	@echo "  bench-build      - 冷/热构建耗时对照"
	@echo "  profile          - 二进制体积与动态依赖"
	@echo "  tag              - 升版本 + 提交 + 打 tag（NEW_VERSION=x.y.z）"
	@echo "  setup-dev        - 安装开发工具链"
	@echo "  help             - 显示本帮助"
