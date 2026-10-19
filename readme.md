# RNU - Rust Nexus Uploader

将 npm / Maven 包批量上传到 Nexus Repository Manager 的 CLI 工具，支持并发、过滤和离线场景。

## 特性

- 🚀 **并发上传**: 可配置的并发数，大幅提升批量上传效率
- 📦 **npm 支持**: 从 `node_modules` 批量扫描、打包、上传
- ☕ **Maven 支持**: 从 `target/` 或 `~/.m2` 扫描 POM/JAR/WAR 并上传
- 🔍 **智能过滤**: `--only` / `--exclude` 按名称筛选
- ⚙️ **配置管理**: 多 profile、CLI 参数覆盖
- 🎯 **精确控制**: `--dry-run` 预览、`--skip-existing` 增量上传
- 📊 **日志分级**: tracing 框架，`-v` = INFO，`RUST_LOG=debug` 可深入诊断
- 🎨 **友好界面**: 彩色输出 + indicatif 进度条

## 安装

### 下载预编译二进制（推荐）

从 [Releases](https://github.com/it-coder/rnu/releases) 取对应平台的包，解包后把 `nexus-upload` 放进 `PATH`。

| 平台 | 架构 | 文件 |
|---|---|---|
| macOS | Apple Silicon | `rnu-<ver>-aarch64-apple-darwin.tar.gz` |
| macOS | Intel | `rnu-<ver>-x86_64-apple-darwin.tar.gz` |
| Linux | x86_64 | `rnu-<ver>-x86_64-unknown-linux-musl.tar.gz` |
| Linux | arm64 | `rnu-<ver>-aarch64-unknown-linux-musl.tar.gz` |
| Windows | x64 | `rnu-<ver>-x86_64-pc-windows-msvc.tar.gz` |

```bash
# 校验（同一 Release 下附 SHA256SUMS）
sha256sum -c SHA256SUMS --ignore-missing
```

几个平台差异：

- **Linux** 发行的是 musl 静态链接包，不依赖 glibc 版本，任何发行版可直接执行。
- **Windows** 是静态链接 CRT 的独立 exe，不需要装 VC++ 运行库。Win10 1803+ 自带 `tar` 可直接解包，或右键用 7-Zip。
- **macOS** 包未做代码签名 / 公证，首次运行会被 Gatekeeper 拦下，需要手动解除隔离：

  ```bash
  xattr -d com.apple.quarantine ./nexus-upload
  ```

### 从源码构建

```bash
git clone <repository-url>
cd rnu

cargo build --release        # → target/release/nexus-upload

# 或使用 Makefile
make release
make install                 # → /usr/local/bin/nexus-upload
make install-user            # → ~/.local/bin/nexus-upload
```

要求：Rust 1.70+

## 快速开始

### 1. 初始化配置

```bash
nexus-upload config init
```

创建 `~/.config/rnu/config.json`。

### 2. 上传 npm 包

```bash
nexus-upload --profile local -p password npm ./my-project
nexus-upload --profile local -p password npm ./my-project --skip-existing --concurrency 8
```

### 3. 上传 Maven 构件

```bash
nexus-upload --profile local -p password -r maven-releases maven ./target
nexus-upload --profile local -p password maven ~/.m2/repository --skip-existing
```

## 命令参考

### 全局参数

| 参数 | 说明 |
|---|---|
| `-s, --server` | Nexus 服务器 URL |
| `-u, --user` | 用户名 |
| `-p, --password` | 密码（必填，不落盘） |
| `-r, --repository` | 仓库名 |
| `--profile` | 使用配置文件中的 profile |
| `--config` | 自定义配置文件路径 |
| `-v, --verbose` | 详细日志（INFO 级别） |
| `--dry-run` | 预览模式，不实际上传 |

### `npm` 子命令

```bash
nexus-upload npm <PATH> [选项]
```

| 选项 | 说明 |
|---|---|
| `--skip-existing` | 跳过 Nexus 中已存在的包 |
| `--only <list>` | 仅上传匹配的包（逗号分隔） |
| `--exclude <list>` | 排除匹配的包 |
| `-c, --concurrency <N>` | 并发上传数（默认 4） |

### `maven` 子命令

```bash
nexus-upload maven <PATH> [选项]
```

接受的目录：Maven `target/`（平铺 POM+JAR）、本地仓库 `~/.m2/repository`（层级结构）、或任意含 `.pom` 文件的目录。

| 选项 | 说明 |
|---|---|
| `--skip-existing` | 跳过 Nexus 中已存在的 artifact |
| `--only <list>` | 仅上传匹配的 artifactId |
| `--exclude <list>` | 排除匹配的 artifactId |
| `-c, --concurrency <N>` | 并发上传数（默认 4） |

### `config` 子命令

```bash
nexus-upload config init              # 初始化配置文件
nexus-upload config show              # 显示当前配置
nexus-upload config list              # 列出所有 profile
nexus-upload config add <NAME> \      # 添加 profile
  --server <URL> --repository <REPO> --user <USER>
```

## 配置文件

位置：`~/.config/rnu/config.json`

```json
{
  "default_server": "http://localhost:8081",
  "default_repository": "npm-hosted",
  "profiles": {
    "local": {
      "server": "http://localhost:8081",
      "repository": "npm-hosted",
      "user": "admin"
    }
  }
}
```

优先级：`CLI 参数 > --profile > default_* > 内置默认值`

## 使用示例

```bash
# npm — 跳过已有 + 过滤 + 并发
nexus-upload --profile prod -p $PASS npm . --skip-existing --only "lodash,react" -c 8

# npm — 干运行预览
nexus-upload --profile local -p pass npm . --dry-run

# Maven — 上传 target 构建产物
nexus-upload --profile local -p pass -r maven-releases maven ./target

# Maven — 从本地仓库扫描
nexus-upload --profile local -p pass maven ~/.m2/repository --skip-existing -c 4

# 日志调试
RUST_LOG=debug nexus-upload --profile local -p pass npm . -v
```

## 开发

```bash
cargo build                           # 调试构建
cargo test                            # 运行测试
cargo run -- npm . --dry-run -v       # 开发运行

make check                            # 全量检查（fmt + clippy + test）
make lint                             # cargo clippy -- -D warnings
```

## 打包与分发

```bash
# 宿主平台
make package
# → dist/rnu-0.1.0-aarch64-apple-darwin.tar.gz 及同名 .sha256 校验和

# 指定其它平台（交叉编译）
make targets                                  # 一次性安装常用 target
make package TARGET=x86_64-apple-darwin
make package TARGET=x86_64-unknown-linux-musl
make package TARGET=x86_64-pc-windows-gnu

# 按 PLATFORMS 清单逐个打包
make package-all

# 升版本（同步 Cargo.toml 与 src/cli.rs，提交并打 tag）
make tag NEW_VERSION=0.2.0
```

产物结构 —— tar 内是单层目录，解包不会散落一地：

```
rnu-0.1.0-aarch64-apple-darwin/
├── nexus-upload          # 可执行文件（Windows 上为 nexus-upload.exe）
├── readme.md
├── config.example.json
└── example_usage.sh
```

### 关于「跨平台」

Rust 编译产出的是**平台相关的原生机器码**，单个二进制无法在多个操作系统上运行。要覆盖多平台，只能为每个平台各出一个包。各平台能从哪里构建：

| 目标 | macOS 宿主 | Linux 宿主 | Windows 宿主 |
|---|---|---|---|
| `aarch64-apple-darwin` | 原生 | 需 cross / CI | 需 CI |
| `x86_64-apple-darwin` | `rustup target add` 即可 | 需 CI | 需 CI |
| `x86_64-unknown-linux-musl` | 需 cross + Docker，或 zig | 原生 | 需 CI |
| `x86_64-pc-windows-gnu` | 需 mingw-w64 | 需 mingw-w64 | 原生 |
| `x86_64-pc-windows-msvc` | 不支持 | 不支持 | 原生 |

需要「一份文件在任何 Linux 发行版上直接跑」时用 `x86_64-unknown-linux-musl`（静态链接，不依赖 glibc 版本）。

### 正式发布：CI 矩阵

正式产物由 CI 构建，不在本机硬啃交叉工具链 —— 每个平台在自己的原生 runner 上编译，产物汇总到同一个 GitHub Release。

```bash
git tag v0.2.0 && git push origin v0.2.0
```

`.github/workflows/release.yml` 会先校验 tag 与 `Cargo.toml` 版本一致、跑格式检查和测试，然后并发构建：

| runner | target | 说明 |
|---|---|---|
| `macos-14` | `aarch64-apple-darwin` | 原生 |
| `macos-14` | `x86_64-apple-darwin` | 交叉（同一工具链，无需额外依赖） |
| `ubuntu-latest` | `x86_64-unknown-linux-musl` | `cross`（镜像自带 musl 工具链） |
| `ubuntu-latest` | `aarch64-unknown-linux-musl` | `cross`（同上） |
| `windows-latest` | `x86_64-pc-windows-msvc` | 原生，静态链接 CRT |

每个 job 都会跑一遍冒烟测试（`--version` / `--help`；跨架构产物改为校验文件格式），musl 包额外确认是静态链接，最后统一生成 `SHA256SUMS` 并上传。手动触发（`workflow_dispatch`）只构建、不发布。

> **为什么 Linux 走 cross**：`ring`（rustls 的加密后端）要编译 C 代码，musl 目标需要 `x86_64-linux-musl-gcc` 这类专用交叉编译器，runner 上并没有。`cross` 的容器镜像里自带全套 musl 工具链，省去逐个目标配 `CC` / 链接器。若本机不想用 Docker，也可以 `apt install musl-tools` 后设置 `CC_x86_64_unknown_linux_musl=musl-gcc`。

> **关于 TLS**：`reqwest` 已切换到 `rustls-tls-native-roots`，不再依赖 OpenSSL，交叉编译不会卡在目标平台的 `libssl` 上；同时仍读取系统信任库，内网 Nexus 的自签 CA 照常有效。

## 技术栈

| 层 | 选型 |
|---|---|
| 异步 | tokio |
| HTTP | reqwest |
| CLI | clap 4.5 |
| 日志 | tracing + tracing-subscriber |
| 错误 | thiserror |
| XML 解析 | quick-xml |
| 压缩 | flate2 + tar |

## License

Apache-2.0