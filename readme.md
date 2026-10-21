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

交互式写入**当前目录**的 `config.toml`（server、用户名、各生态的仓库名、可选密码），文件权限为 `600`。
配置文件跟着项目走：在哪个目录执行，就读哪份配置，不需要的项目互不干扰。
非交互环境（CI）直接用全局参数或环境变量：

```bash
export RNU_SERVER=http://nexus.company.com:8081
export RNU_USER=deployer
export RNU_PASSWORD=***        # 推荐：密码不落盘
```

### 2. 上传 npm 包

```bash
nexus-upload npm ./my-project
nexus-upload npm ./my-project --skip-existing -c 8
```

### 3. 上传 Maven 构件

`maven` 只有一个位置参数，来源由内容自动识别：

```bash
# 项目目录（含 pom.xml）：用 mvn 把依赖解析到按项目隔离的缓存仓库，再全部上传
# 不碰 ~/.m2，也不会带上别的项目
nexus-upload maven ./my-app

# 连本项目自己的构建产物一起传（内部改为 mvn install）
nexus-upload maven ./my-app --with-project

# 已有的仓库布局目录
nexus-upload maven ~/.m2/repository --skip-existing

# 松散目录（一堆 pom/jar 平铺在一起，如 target/）
nexus-upload maven ./target

# 识别不准时显式指定
nexus-upload maven ./target --layout loose
```

项目模式的执行链路：

```
mvn -B -Dmaven.repo.local=<缓存目录> dependency:go-offline
  └─ 逐行读取 mvn 输出，用 spinner 显示当前在下载什么（不再静默）
  └─ 扫描该目录（仓库布局）→ 每个 artifact 连同 classifier 一起上传
```

坐标来自路径而不是解析 XML，因此 `-sources.jar` / `-javadoc.jar` 会带上正确的 classifier，不会冒充主 jar。

**仓库按项目缓存**：落在**当前目录**的 `.rnu/m2/<项目哈希>`，和 `config.toml` 放在一起——换工作目录就是换一套缓存，`rm -rf .rnu` 即可回收全部空间，也不会往家目录里堆东西。第二次运行直接复用，不用重新下载几百 MB。

`pom.xml` 内容一变就自动作废重解（避免旧版本构件被当成「本项目依赖」上传）；`--fresh` 强制重解；`--keep-repo <dir>` 用固定目录替代 `.rnu/m2/<hash>`（想跨工作目录共享缓存时用，注意它会跨项目累积构件）。当前目录不可写时退回临时目录并给出提示，本次不上传缓存。

在项目根目录跑的话，记得把 `.rnu/` 加进该项目的 `.gitignore`。

首次解析会下载整个闭包（Spring Boot 系项目通常几百 MB、几分钟），期间 spinner 会显示 `Downloading from …`，不是卡死。

Maven 的 `settings.xml` **原样使用**：项目可能依赖只存在于私库的内部构件，切断私库会导致解析失败。私库里已有的构件由 `--skip-existing` 挡掉。

## 命令参考

所有全局参数既可写在子命令前，也可写在子命令后（`nexus-upload npm . -s URL` 与 `nexus-upload -s URL npm .` 等价）。

### 全局参数

| 参数 | 环境变量 | 说明 |
|---|---|---|
| `-s, --server <URL>` | `RNU_SERVER` | Nexus 服务器 URL |
| `-u, --user <USER>` | `RNU_USER` | 用户名 |
| `-p, --password` | `RNU_PASSWORD` | 密码 |
| `-r, --repository` | `RNU_REPO` | 仓库名（不填则按生态取配置文件里的默认值） |
| `--profile <NAME>` | | 使用配置文件中的 profile |
| `--config <PATH>` | | 配置文件路径（默认 `./config.toml`；`.json` 结尾仍按 JSON 读写） |
| `-v, --verbose` | | 详细日志（INFO 级别） |
| `--dry-run` | | 预览模式，不实际上传 |
| `-c, --concurrency <N>` | | 并发上传数（默认 4） |
| `--skip-existing` | | 跳过 Nexus 中已存在的包 |
| `--only <list>` | | 仅上传匹配的包（逗号分隔） |
| `--exclude <list>` | | 排除匹配的包（逗号分隔） |

### `npm` 子命令

```bash
nexus-upload npm <PATH>
```

`PATH` 可以是项目根目录（自动找 `node_modules`）或 `node_modules` 本身。

### `maven` 子命令

```bash
nexus-upload maven <PATH> [--layout auto|project|repo|loose]
```

| `PATH` 的形态 | 识别结果 | 行为 |
|---|---|---|
| 目录含 `pom.xml`，或直接传 `pom.xml` | `project` | 用 `mvn` 解析依赖闭包到缓存仓库后上传 |
| `<group>/<artifact>/<version>/…` | `repo` | 扫描仓库布局，坐标取自路径 |
| 其它含 `.pom` 的目录 | `loose` | 解析 POM XML 取坐标，匹配同目录构件 |

`--layout` 可跳过识别直接指定。项目模式专属选项：

| 选项 | 说明 |
|---|---|
| `--with-project` | 连本项目产物一起传（`install` 而非 `go-offline`） |
| `--keep-repo <DIR>` | 用指定目录替代 `.rnu/m2/<hash>` |
| `--fresh` | 忽略缓存，强制重新解析依赖 |
| `--mvn <PATH>` | 指定 `mvn` 可执行文件（也支持 `RNU_MVN`） |

### `config` 子命令

```bash
nexus-upload config init              # 在当前目录生成 config.toml（终端里交互式）
nexus-upload config init --from OLD   # 从旧配置迁移（TOML 或 JSON）
nexus-upload config show              # 显示当前配置（密码打码）
nexus-upload config list              # 列出默认值与所有 profile
nexus-upload -s URL -u USER config add corp \   # 添加 profile
  --npm-repo npm-internal --maven-repo maven-releases
```

## 配置文件

位置：**当前目录**下的 `config.toml`（UNIX 下权限 `600`）。`--config <PATH>` 可指向别处，`.json` 结尾的路径仍按 JSON 读写，老配置直接能用。

```toml
default_server = "http://localhost:8081"
default_user = "admin"
# default_password = ""          # 不建议落盘，优先 RNU_PASSWORD

[repositories]
npm = "npm-hosted"
maven = "maven-releases"

[profiles.corp]
server = "https://nexus.company.com"
user = "deployer"

[profiles.corp.repositories]
npm = "npm-internal"
maven = "maven-releases"
```

优先级：`CLI 参数 > 环境变量 > --profile > 配置文件默认值`。

仓库名按生态解析：`maven` 没有内置默认值——没配置就报错，而不是猜一个（早期版本会把 Maven 构件静默指向 `npm-hosted`）。旧配置里的 `default_repository` / `profile.repository` 仍然有效，但只作用于 npm。

> **旧版（≤ 0.2.0）的全局配置**（`~/.config/rnu/config.json`，macOS 上是 `~/Library/Application Support/rnu/config.json`）不再读取。迁移：`nexus-upload config init --from "<旧路径>"`，字段原样带过来，JSON 也会转成 TOML 写法。
>
> 配置文件跟着项目走，含密码时记得把它排除在版本库外，或在 CI 里改用 `RNU_PASSWORD`。

## 使用示例

```bash
# 配置文件就绪后，日常命令只有一个路径
nexus-upload npm . --skip-existing --only "lodash,react" -c 8
nexus-upload maven ./my-app --dry-run

# CI：密码走环境变量，不落盘、不进 ps
RNU_PASSWORD="$NEXUS_PASS" nexus-upload maven ./my-app -r maven-releases

# 用 profile 切换环境
nexus-upload --profile corp maven ./my-app

# 日志调试
RUST_LOG=debug nexus-upload npm . -v
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
├── config.example.toml
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