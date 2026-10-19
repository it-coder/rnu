# RNU - Rust Nexus Uploader 项目总结

## 项目概述

RNU (Rust Nexus Uploader) 是一个基于 Rust 开发的 CLI 工具，支持将 npm 包和 Maven 构件批量上传到 Nexus Repository Manager 3.x。适用于离线环境、企业内网、CI/CD 流水线等场景。

二进制名：`nexus-upload`

## 当前实现 (v0.1.0)

### npm 包批量上传
- 扫描 `node_modules` 目录，解析 `package.json`
- 支持 scoped 包（`@types/node`）
- 打 tar.gz 后 PUT 到 Nexus
- `--skip-existing` 并发 HEAD 检查，三阶段流水线（扫描→过滤→上传）
- `--only` / `--exclude` 过滤，`-c` 并发数

### Maven 构件上传
- 递归扫描目录树中的 `.pom` 文件
- 使用 quick-xml 解析 POM：提取 `groupId` / `artifactId` / `version` / `packaging`
- 支持 `<parent>` 继承坐标
- 自动匹配同目录的 `.jar` / `.war` / `.aar` 文件
- 通过 `maven2.*` multipart 表单上传（pom + jar + war 多 asset）
- `--skip-existing` / `--only` / `--exclude` / `--concurrency` 均支持

### 配置管理
- JSON 配置文件（`~/.config/rnu/config.json`）
- 多 profile（`--profile`）
- CLI > profile > 默认值 优先级合并（`ConfigResolver`）
- 密码不落盘，必须 `-p` 传入

### CLI
- clap derive，子命令 `npm` / `maven` / `config` / `pypi`(隐藏)
- `--dry-run` 预览、`-v` tracing 日志、`--concurrency` 并发

## 项目结构

```
src/
├── main.rs           CLI 编排入口
├── cli.rs            clap 数据定义
├── config.rs         配置层（Config + ConfigFile + ConfigResolver）
├── error.rs          错误类型（thiserror + From<quick_xml::Error>）
├── nexus/
│   ├── mod.rs        编排器（NexusUploader） + 类型 + 测试
│   ├── client.rs     HTTP 客户端（npm/maven 上传、存在性检查、超时+UA）
│   ├── scanner.rs    npm 扫描器（同步，无网络）
│   └── maven.rs      Maven 扫描器 + POM 解析
└── utils/
    ├── mod.rs        工具函数
    └── progress.rs   进度条
```

### 上传流水线（npm / maven 统一模式）

```
Phase 1 (sync)   → 扫描文件系统，收集所有包/构件
Phase 2 (async)  → --only/--exclude 过滤
Phase 3 (async)  → 如果 --skip-existing，并发 HEAD 查询并过滤
Phase 4 (async)  → 并发上传（JoinSet + Semaphore），统一进度和错误汇总
```

## 技术栈

| 层 | 选型 |
|---|---|
| 异步运行时 | tokio（rt-multi-thread + macros） |
| HTTP | reqwest（30s 超时 + UA） |
| CLI | clap 4.5 (derive) |
| 日志 | tracing + tracing-subscriber（env-filter） |
| 序列化 | serde + serde_json |
| XML 解析 | quick-xml |
| 进度条 | indicatif |
| 错误 | thiserror |
| 压缩 | flate2 + tar |

## 使用示例

```bash
# npm — 并发 8 路 + 跳过已有
nexus-upload --profile prod -p $PASS npm . --skip-existing -c 8

# npm — 过滤
nexus-upload --profile local -p pass npm . --only "lodash,react"

# Maven — 上传 target
nexus-upload --profile local -p pass -r maven-releases maven ./target -c 4

# Maven — 本地仓库扫描 + 跳过已有
nexus-upload --profile local -p pass maven ~/.m2/repository --skip-existing

# 干运行
nexus-upload --profile local -p pass npm . --dry-run

# 配置管理
nexus-upload config init
nexus-upload config add staging --server https://nx.example.com --repository npm-private --user deploy
```

## 打包与分发

打包统一走 Makefile，产物落在 `dist/`：

```bash
make package                                  # 宿主平台
make package TARGET=x86_64-unknown-linux-musl # 指定平台（交叉）
make package-all                              # 按 PLATFORMS 清单逐个打
make targets                                  # rustup target add 常用平台
make tag NEW_VERSION=0.2.0                    # 升版本 + 提交 + 打 tag
```

产物命名带平台标识 `rnu-<version>-<target-triple>.tar.gz`，并附 `.sha256`；tar 内是单层目录（`nexus-upload` / `readme.md` / `config.example.json` / `example_usage.sh`），解包不会散落。

Makefile 里所有平台相关路径由 `TARGET` / `EXE` / `RELEASE_DIR` 派生，Windows 上自动使用 `nexus-upload.exe` 与 `target/<triple>/release/`。

**跨平台约束**：Rust 编译产物是平台相关的原生机器码，单个二进制无法跨操作系统运行，只能按平台分别出包。本机交叉能力取决于工具链 —— macOS 可交叉出两个 Apple 架构，Windows 目标需 mingw-w64，Linux 目标需 cross + Docker 或 zig。

**正式发布走 CI**：`.github/workflows/release.yml` 在推送 `v*` tag 时触发，5 个平台各自在原生 runner 上构建（macOS 双架构 / Linux musl 双架构 / Windows msvc），跑冒烟测试后生成统一 `SHA256SUMS`，汇总到同一个 GitHub Release。手动触发只构建不发布。

`reqwest` 已切换到 `rustls-tls-native-roots`，不再依赖 OpenSSL，交叉编译不再受目标平台 `libssl` 制约；同时仍读取系统信任库，内网 Nexus 的自签 CA 照常有效。Windows 与 Linux musl 目标在 `.cargo/config.toml` 里开启 `crt-static`，产物自包含（Windows 无需 VC++ 运行库，Linux 不依赖 glibc 版本）。

## 未来规划

- [ ] PyPI 包支持
- [ ] `--include-dev` 实现（npm devDependencies 过滤）
- [ ] 上传失败重试
- [ ] 配置文件 `config remove` 子命令
- [x] 多平台发布走 CI matrix（各平台原生 runner 构建，规避交叉工具链）
- [x] 将 `native-tls` 换为 `rustls-tls-native-roots`，降低交叉编译门槛（保留系统信任库，企业自签 CA 不受影响）
- [ ] 修复 scoped 包名被截断（`@types/node` → `node`）导致 `--only` / `--skip-existing` 失效
- [ ] 修复 Maven `-sources.jar` / `-javadoc.jar` 缺少 `classifier` 被当作主 jar 上传
- [ ] 将 `src/nexus/mod.rs` 测试中的硬编码地址与明文密码改为环境变量

## License

Apache-2.0