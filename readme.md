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

```bash
git clone <repository-url>
cd rnu

# 构建
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