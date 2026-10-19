# CLAUDE.md

本文档为在本仓库工作的 AI 编码助手提供上下文。架构约束、编码约定见本文末。

---

## 仓库现状

**rnu** — Rust Nexus Uploader，单 crate 的 CLI 二进制工具。将本地 npm / Maven 包批量上传到 Nexus Repository Manager 3.x。

**实现状态：**
- ✅ npm 包批量上传（`nexus-upload npm <path>`）：三阶段流水线（同步扫描 → 并发 HEAD 过滤 → 并发上传）
- ✅ Maven 构件上传（`nexus-upload maven <path>`）：POM XML 解析 → 匹配 JAR/WAR → 并发上传
- ❌ pypi：子命令隐藏（`hide = true`），未实现
- 错误处理：`thiserror` derive，含 `From<quick_xml::Error>`
- 异步运行时：`tokio`（`#[tokio::main]`，`JoinSet` + `Semaphore` 并发）
- 日志：`tracing` + `tracing-subscriber`（env-filter），`-v` = INFO，`RUST_LOG=debug` 可深入
- HTTP 客户端：`reqwest` 非 blocking，30s 超时，自定义 UA；TLS 走 **rustls**（`rustls-tls-native-roots`），不依赖 OpenSSL

**已知问题：**
- `Cargo.toml` license = `Apache-2.0`，仓库根目录已有 `LICENSE` 全文（Apache-2.0）
- 版本号硬编码在**两处**：`Cargo.toml` 与 `src/cli.rs` 的 clap `version`。clap 显式 version 会覆盖 `CARGO_PKG_VERSION`，只改 `Cargo.toml` 会让 `nexus-upload --version` 停留在旧版本 —— 升版本请走 `make tag NEW_VERSION=x.y.z`；CI 会校验 tag 与 `Cargo.toml` 一致
- `tests/nexus_test.rs` 为集成测试占位文件（后续需 docker-compose Nexus 环境）
- `anyhow` 已移除；`thiserror` / `tokio` 已激活；`tracing` / `quick-xml` 已引入
- `src/nexus/mod.rs` 的 `test_config()` 里**硬编码了真实服务器地址与明文密码**，`test_upload_tarball` 硬编码了本机绝对路径，待改为环境变量
- `src/utils/mod.rs` 除 `progress` 之外全部是死代码；`UploadProgress::increment_skipped` 从未被调用，`--skip-existing` 跳过的包不计入统计
- `docs/` 目录已存在但为空，后续按需建立子目录
- `make check`（含 `clippy -D warnings`）当前**会失败**，仓库里有 14 条 dead_code 等告警待清理 —— 因此 CI 门禁只跑 `fmt --check` 与 `cargo test`，clippy 尚未纳入
- `PROJECT_SUMMARY.md` / `readme.md` 为面向用户文档，改功能后同步更新

**跨平台构建要点：**
- 代码本身无平台分支：路径统一 `PathBuf`，配置目录用 `dirs::config_dir()`
- 正式产物由 `.github/workflows/release.yml` 在 5 个平台上构建（Linux musl 两个目标走 `cross` 容器，因为 `ring` 需要 musl 专用 C 交叉编译器），不依赖本机交叉工具链
- `.cargo/config.toml` 为 Windows（msvc/gnu）与 Linux musl 目标开启 `crt-static`，产物自包含
- 本机能直接打的只有 macOS 两个 arch；Linux/Windows 请走 CI

---

## 目录地图

```
rnu/
├── src/
│   ├── main.rs           CLI 编排入口：parse → tracing init → config merge → 分发子命令
│   ├── cli.rs            clap 数据定义（Cli / Commands / ConfigAction）
│   ├── config.rs         配置层：Config + ConfigFile + ProfileConfig + ConfigResolver
│   ├── error.rs          错误类型：RnuError（thiserror），含 From<quick_xml::Error>
│   ├── nexus/
│   │   ├── mod.rs        编排器 + PackageEntry/FilterOptions 类型 + 测试
│   │   ├── client.rs     NexusClient（HTTP：npm/maven 上传、存在性检查）
│   │   ├── scanner.rs    PackageScanner（npm 文件系统扫描，同步）
│   │   └── maven.rs      MavenScanner + POM XML 解析 + MavenArtifact
│   └── utils/
│       ├── mod.rs        工具函数（部分 dead code，待清理）
│       └── progress.rs   indicatif 进度条封装
├── tests/
│   └── nexus_test.rs     集成测试占位
├── .github/
│   └── workflows/
│       └── release.yml   打 v* tag → 5 平台构建 → 汇总到 GitHub Release
├── .cargo/
│   └── config.toml       交叉目标的 rustflags（Windows/Linux musl 静态链接）
├── docs/                 设计决策/路线图/工程笔记/用户文档
├── examples/
│   └── example_usage.sh  使用示例脚本
├── Cargo.toml            二进制名 nexus-upload，edition 2021
├── rust-toolchain.toml   固定 stable 渠道 + rustfmt/clippy
├── Makefile              构建/测试/安装/打包
├── config.example.json   配置文件模板
├── LICENSE               Apache-2.0 全文
├── readme.md             用户文档
├── PROJECT_SUMMARY.md    项目总结（改代码后同步更新）
├── AGENTS.md             指向本文件
└── CLAUDE.md             本文件
```

### 核心流程（npm / Maven 统一三阶段）

```
nexus-upload npm/maven <path>
  └─ Cli::parse()
       └─ init_tracing(-v)
            └─ build_config() → ConfigResolver::resolve()
                 └─ Config::validate()
                      └─ Arc<NexusUploader>::run() / run_maven()
                           ├─ Phase 1: PackageScanner/MavenScanner::scan()         # 同步
                           ├─ Phase 2: FilterOptions::should_include()             # 过滤
                           ├─ Phase 3: filter_existing() → 并发 HEAD              # skip_existing
                           └─ Phase 4: upload_batch() → JoinSet + Semaphore       # 并发上传
```

### 配置优先级

```
CLI (-s/-u/-p/-r)  >  --profile 指定的 profile  >  配置文件 default_*  >  内置默认值
```

密码 **必须** 通过 `-p` 传入，不写入配置文件，不从 profile 读取。

---

## 常用命令

```bash
# 开发
cargo build
cargo run -- --help
cargo run -- npm . --dry-run -v
cargo run -- maven ./target --dry-run -v      # Maven 干运行

# 发布
cargo build --release
make release

# 正式发版：升版本 → 打 tag → 推 tag（CI 构建 5 个平台并发布 Release）
make tag NEW_VERSION=0.2.0
git push origin master --tags

# 打包（TARGET 留空 = 宿主平台；产物在 dist/，附 .sha256）
make package
make package TARGET=x86_64-unknown-linux-musl
make targets          # rustup target add 常用平台
make package-all      # 按 PLATFORMS 逐个打（需交叉工具链）

# 升版本（同步 Cargo.toml + src/cli.rs 的 clap version，提交并打 tag）
make tag NEW_VERSION=0.2.0

# 测试
cargo test
cargo test -- --nocapture

# 检查（提交前必跑）
make check     # fmt + clippy(-D warnings) + test
make lint      # cargo clippy -- -D warnings
make format    # cargo fmt

# 安全审计
make check-deps  # cargo audit

# 安装
make install       # sudo cp → /usr/local/bin/
make install-user  # cp → ~/.local/bin/

# 依赖
make deps     # cargo tree
make update   # cargo update
```

---

## 编码约定

1. **错误处理**：统一用 `error::Result<T>`。`RnuError` 用 `thiserror::Error` derive。新增外部 crate 的错误类型时添加 `From` impl。
2. **异步**：`#[tokio::main]`，所有 HTTP I/O 用 async。并发使用 `JoinSet` + `Semaphore`（不引入 `futures` crate）。
3. **模块组织**：新功能放 `src/nexus/` 子模块，在 `mod.rs` 中通过 `NexusUploader` 编排。公共类型在 `mod.rs` 顶部定义。
4. **CLI**：clap derive 模式，新子命令加在 `cli.rs` 的 `Commands` enum。`--concurrency` 参数命名统一为 `-c`。
5. **配置**：密码不走文件，不做 profile 的 password 字段。
6. **日志**：用户可见输出用 `println!`/`eprintln!`（彩色）。诊断输出用 `tracing` 宏（`info!`/`debug!`/`warn!`/`error!`）。
7. **提交信息**：中文，简明描述改动。
8. **交流**：用中文。
9. **文档**：改代码后同步更新 `PROJECT_SUMMARY.md` 和 `readme.md`。`docs/` 目录后续按需创建：`design/`（设计决策）、`plan/`（路线图）、`notes/`（工程笔记，含 `crate-notes.md`）、`user/`（用户文档）。
10. **打包**：一律走 `make package`，不要手写 `tar`/`cp` 流程。Makefile 里与平台相关的路径都通过 `TARGET` / `EXE` / `RELEASE_DIR` 派生，新增 recipe 时不要硬编码 `target/release/nexus-upload` —— Windows 上产物是 `nexus-upload.exe`。Makefile 与 `examples/` 下的脚本要同时兼容 GNU Make 3.81、macOS 的 BSD 工具链与 MSYS2；新增 shell 代码前先确认它是不是 POSIX（例如 `echo -e` 在 dash 下会把 `-e` 当字面文本打印出来，应改用 `printf`）。
