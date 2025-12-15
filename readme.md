# RNU - Rust Nexus Uploader

一个用于上传npm包到Nexus仓库的高性能CLI工具，专门为离线环境和批量上传设计。

## 特性

- 🚀 **高性能**: 基于Rust开发，支持并发上传
- 📦 **离线支持**: 从本地node_modules目录批量上传npm包
- 🔍 **智能过滤**: 支持包含/排除特定包
- ⚙️ **配置管理**: 支持配置文件和多环境配置
- 🎯 **精确控制**: 支持干运行模式和详细日志
- 🔄 **增量上传**: 自动跳过已存在的包
- 🎨 **友好界面**: 彩色输出和进度条显示

## 安装

### 从源码构建

```bash
git clone <repository-url>
cd rnu
cargo build --release
```

构建完成后，可执行文件位于 `target/release/nexus-upload`

### 添加到PATH

```bash
# Linux/macOS
export PATH=$PATH:/path/to/rnu/target/release

# 或者复制到系统目录
sudo cp target/release/nexus-upload /usr/local/bin/
```

## 快速开始

### 1. 初始化配置

```bash
nexus-upload config init
```

这将在 `~/.config/rnu/config.json` 创建配置文件模板。

### 2. 编辑配置文件

```json
{
  "default_server": "http://localhost:8081",
  "default_repository": "npm-hosted",
  "profiles": {
    "local": {
      "server": "http://localhost:8081",
      "repository": "npm-hosted",
      "user": "admin"
    },
    "production": {
      "server": "https://nexus.company.com",
      "repository": "npm-private",
      "user": "deploy-user"
    }
  }
}
```

### 3. 上传包

```bash
# 基本用法 - 上传项目的node_modules
nexus-upload -s http://localhost:8081 -u admin -p password npm ./my-project

# 使用配置文件中的profile
nexus-upload --profile local -p password npm ./my-project

# 直接指定node_modules目录
nexus-upload --profile local -p password npm ./my-project/node_modules
```

## 命令详解

### 主要参数

- `-s, --server <URL>`: Nexus服务器地址
- `-u, --user <USERNAME>`: 用户名
- `-p, --password <PASSWORD>`: 密码
- `-r, --repository <REPO>`: 仓库名称 (默认: npm-hosted)
- `--profile <NAME>`: 使用配置文件中的profile
- `-v, --verbose`: 详细输出
- `--dry-run`: 干运行模式，显示将要上传的包但不实际上传

### NPM子命令

```bash
nexus-upload [OPTIONS] npm [NPM_OPTIONS] <PATH>
```

NPM选项:
- `--skip-existing`: 跳过已存在的包
- `--include-dev`: 包含开发依赖
- `--only <PACKAGES>`: 仅上传指定包 (逗号分隔)
- `--exclude <PACKAGES>`: 排除指定包 (逗号分隔)

### 配置管理

```bash
# 初始化配置文件
nexus-upload config init

# 显示当前配置
nexus-upload config show

# 列出所有profile
nexus-upload config list

# 添加新的profile
nexus-upload config add production \
  --server https://nexus.company.com \
  --repository npm-private \
  --user deploy-user
```

## 使用示例

### 基本上传

```bash
# 上传当前项目的所有依赖
nexus-upload -s http://localhost:8081 -u admin -p admin123 npm .

# 上传时跳过已存在的包
nexus-upload -s http://localhost:8081 -u admin -p admin123 npm . --skip-existing
```

### 使用过滤器

```bash
# 只上传lodash和express相关的包
nexus-upload --profile local -p password npm . --only "lodash,express"

# 排除测试相关的包
nexus-upload --profile local -p password npm . --exclude "test,mock,jest"
```

### 干运行模式

```bash
# 查看将要上传什么包，但不实际上传
nexus-upload --profile local -p password npm . --dry-run
```

### 详细模式

```bash
# 显示详细的上传过程
nexus-upload --profile local -p password npm . -v
```

## 配置文件

### 位置

- Linux/macOS: `~/.config/rnu/config.json`
- Windows: `%APPDATA%/rnu/config.json`

### 配置结构

```json
{
  "default_server": "默认服务器URL",
  "default_repository": "默认仓库名",
  "profiles": {
    "profile名称": {
      "server": "服务器URL",
      "repository": "仓库名 (可选)",
      "user": "用户名 (可选)"
    }
  }
}
```

### 配置优先级

1. 命令行参数 (最高优先级)
2. 指定的profile配置
3. 配置文件默认值
4. 内置默认值 (最低优先级)

## 支持的包格式

目前支持:
- ✅ npm包 (.tgz, .tar.gz)

计划支持:
- 🔄 Maven包 (.jar, .war, .pom)
- 🔄 PyPI包 (.whl, .tar.gz)
- 🔄 Docker镜像

## 错误处理

工具会自动处理常见错误:

- **网络连接问题**: 自动重试机制
- **认证失败**: 清晰的错误提示
- **包不存在**: 跳过并继续处理其他包
- **权限不足**: 详细的权限错误说明

## 性能优化

- 并发上传以提高速度
- 智能去重避免重复上传
- 进度条显示实时状态
- 内存高效的文件处理

## 开发

### 构建要求

- Rust 1.70+
- Cargo

### 运行测试

```bash
cargo test
```

### 开发模式运行

```bash
cargo run -- --help
```

## 贡献

欢迎提交Issues和Pull Requests。

## 许可证

MIT OR Apache-2.0

## 更新日志

### v0.1.0

- 初始版本
- 支持npm包上传
- 配置文件管理
- 基本过滤功能

## 常见问题

### Q: 如何处理大量包的上传？

A: 使用 `--skip-existing` 参数可以跳过已存在的包，大大减少上传时间。同时 `-v` 参数可以显示详细进度。

### Q: 密码安全性如何保障？

A: 密码必须通过命令行参数传入，不会存储在配置文件中。建议使用环境变量或安全的密码管理工具。

### Q: 支持哪些Nexus版本？

A: 支持Nexus Repository Manager 3.x 版本。

### Q: 如何处理网络超时？

A: 工具内置重试机制，可以通过 `-v` 参数查看详细的网络请求信息。