# CLAUDE.md

本文档为在本仓库工作的 AI 编码助手提供上下文。架构约束、编码约定见本文末。

---

## 仓库现状

**rnu** — Rust Nexus Uploader，单 crate 的 CLI 二进制工具。将本地 npm / Maven 包批量上传到 Nexus Repository Manager 3.x。

**实现状态：**
- ✅ npm 包批量上传（`nexus-upload npm <path>`）：三阶段流水线（同步扫描 → 并发 HEAD 过滤 → 并发上传）
- ✅ Maven 构件上传，**单一位置参数 + 自动识别布局**（`MavenLayout::detect`），`--layout` 可覆盖：
  - `project`：目录含 `pom.xml`（或直接传该文件）→ `mvn -Dmaven.repo.local=<临时仓库> dependency:go-offline`
    → 扫描该临时仓库 → 上传。隔离仓库保证只上传本项目实际解析到的依赖；`--with-project` 改用 `install`
    把本项目产物也纳入。`settings.xml` 原样使用（项目可能依赖只在私库存在的内部构件）
  - `repo`：`<group>/<artifact>/<version>/…` → **坐标取自路径**，classifier 取自文件名，无需解析 XML
  - `loose`：其它含 `.pom` 的目录 → 解析 XML 取坐标 + 同目录伴生文件
- ✅ CLI 形态：全局参数均 `global = true`（可写在子命令前后）且支持环境变量
  `RNU_SERVER` / `RNU_USER` / `RNU_PASSWORD` / `RNU_REPO` / `RNU_MVN`
- ❌ pypi：子命令隐藏（`hide = true`），未实现
- 错误处理：`thiserror` derive，含 `From<quick_xml::Error>`
- 异步运行时：`tokio`（`#[tokio::main]`，`JoinSet` + `Semaphore` 并发）
- 日志：`tracing` + `tracing-subscriber`（env-filter），`-v` = INFO，`RUST_LOG=debug` 可深入
- HTTP 客户端：`reqwest` 非 blocking，30s 超时，自定义 UA；TLS 走 **rustls**（`rustls-tls-native-roots`），不依赖 OpenSSL

**已知问题：**
- `Cargo.toml` license = `Apache-2.0`，仓库根目录已有 `LICENSE` 全文（Apache-2.0）
- 版本号单一来源：`#[command(version)]` 取 `CARGO_PKG_VERSION`，只在 `Cargo.toml` 改版本即可（`make tag NEW_VERSION=x.y.z`）；CI 会校验 tag 与 `Cargo.toml` 一致
- `tests/nexus_test.rs` 为集成测试占位文件（后续需 docker-compose Nexus 环境）
- `anyhow` 已移除；`thiserror` / `tokio` 已激活；`tracing` / `quick-xml` 已引入
- `src/nexus/mod.rs` 的 `test_config()` 里**硬编码了真实服务器地址与明文密码**，`test_upload_tarball` 硬编码了本机绝对路径，待改为环境变量
- `src/utils/mod.rs` 除 `progress` 之外全部是死代码；`UploadProgress::increment_skipped` 目前只在 Maven 路径被调用，npm 的 `--skip-existing` 跳过项仍不计入统计
- `npm --include-dev` 已标记为无效（`hide = true`，用了会打警告）：scanner 本来就扫全部 `node_modules`，这个开关从来没有实际作用
- `docs/plan/roadmap.md` 记录了待修缺陷与排期（Maven POM 坐标解析错乱、scoped 包名截断等）
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
│   ├── main.rs           CLI 编排入口：parse → tracing init → layout 识别 → config merge → 分发
│   ├── cli.rs            clap 数据定义（Cli / Commands / LayoutArg / ConfigAction）
│   ├── config.rs         配置层：Kind + Config + ConfigFile + ProfileConfig + ConfigResolver
│   ├── error.rs          错误类型：RnuError（thiserror），含 From<quick_xml::Error>
│   ├── mvn.rs            调用外部 mvn，把依赖解析进隔离仓库（project 布局用）
│   ├── nexus/
│   │   ├── mod.rs        编排器 + PackageEntry/FilterOptions 类型 + 测试
│   │   ├── client.rs     NexusClient（HTTP：npm/maven 上传、存在性检查）
│   │   ├── scanner.rs    PackageScanner（npm 文件系统扫描，同步）
│   │   └── maven.rs      MavenLayout（自动识别）+ MavenScanner（松散 POM）
│   │                     + MavenRepoScanner（仓库布局）
│   │                     + MavenArtifact/MavenFile（含 classifier）+ POM XML 解析
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
  └─ Cli::parse()                                    # global=true，参数可写在子命令前后
       └─ init_tracing(-v)
            └─ build_config(kind) → ConfigResolver::resolve()
                 └─ Config::validate()
                      └─ Arc<NexusUploader>::run() / run_maven(_repo|_project)()
                           ├─ Phase 1: PackageScanner/MavenScanner::scan()         # 同步
                           ├─ Phase 2: FilterOptions::should_include()             # 过滤
                           ├─ Phase 3: filter_existing() → 并发 HEAD              # skip_existing
                           └─ Phase 4: upload_batch() → JoinSet + Semaphore       # 并发上传
```

### 配置优先级

```
CLI 参数  >  环境变量（RNU_SERVER/USER/PASSWORD/REPO）  >  --profile  >  配置文件 default_*  >  npm 内置默认值
```

环境变量由 clap 的 `env` 属性合并进 `Cli` 结构，优先级天然高于配置文件、低于显式参数。

**仓库名按生态解析**（`ConfigResolver::resolve_repository`）：`repositories.<kind>` 是主路径；
旧的 `default_repository` / `profile.repository` 只对 npm 生效。**maven 没有内置默认值**——未配置就报错，
不要加回 `npm-hosted` 兜底（那会让 Maven 构件静默指向 npm 仓库）。

密码可来自 `-p` / `RNU_PASSWORD` / `profile.password` / `default_password`；若写入配置文件，
`ConfigFile::save_to_file` 会 chmod 600，`config show` 一律打码。

### Maven 布局识别（`MavenLayout::detect`）

按顺序判定，任一步命中即返回：

1. 路径是 `pom.xml` 文件 → `project`（其它文件直接报错）
2. 目录下存在 `pom.xml` → `project`（项目目录里也有 `.pom`，所以必须先判）
3. 递归扫描扩展名为 jar/war/aar/zip/pom 的文件，能被 `classify_in_repo` 解析出 `group/artifact/version`
   的占比 ≥ 50% → `repo`（命中 32 个后提前结束，避免在大仓库上全量遍历）
4. 其余 → `loose`

`--layout` 可跳过判定。判定逻辑在 `src/nexus/maven.rs`，改动时同步更新那里的 `detect_*` 测试。

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
