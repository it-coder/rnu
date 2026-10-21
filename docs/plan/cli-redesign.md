# CLI 简化方案：对照 dep-porter（gudaoxuri/dep-downloader）

> 依据：Docker Hub `gudaoxuri/dep-downloader` 页面（dep-porter）公开文档；
> 其 import 阶段的确切参数名、是否支持清单批量文件 **未能核实**，文中已标注。
> rnu 侧数据为本机实测（v0.2.0，2026-10-21）。

## 1. dep-porter 是什么

两段式跨网搬运工具：**外网机 download → 目录为介质 → 内网机 import → Nexus**。

- 生态 5 种：Maven（`mvn dependency:get` 递归）、npm（打包依赖树）、PyPI（`pip download`）、
  Cargo（复制 `.crate`）、Conan（`conan install`，Nexus 原生不支持，走 raw 兜底）
- 命令形态：`dep-porter download --kind maven --name org.apache.commons:commons-lang3 --version 3.14.0`
- 下载阶段自动拉 Docker 镜像执行，本机不需要 mvn/npm/python
- 默认做安全漏洞检查（`--no-check-security` 关闭）
- 导入端纯 HTTP（npm 用原生 publish 协议、Cargo 用 `PUT /api/v1/crates/new`，仅 PyPI 依赖 twine）
- 镜像源走环境变量（`MAVEN_MIRROR` 等，默认国内源）

## 2. 功能对照

| 维度 | dep-porter | rnu 现状 |
|---|---|---|
| 定位 | 跨网搬运（两端可物理隔离） | 直连 Nexus 的批量上传器 |
| 阶段 | 两段，产物是可搬运目录 | 一段，扫描/解析后直传 |
| 生态 | 5 种 | 2 种（npm、maven；pypi 是隐藏空壳） |
| 输入粒度 | **单个坐标**（一次一包） | 目录 / 项目 / 仓库布局（一次几千包） |
| 传递依赖 | 下载阶段自动展开 | `maven --project` 展开；npm 靠已装好的 node_modules |
| 本机工具链 | 只需 Docker | 需要 mvn（`--project`）、需要已 install 的 node_modules |
| 认证 | 环境变量 | 命令行 5 个连接参数；配置文件**存不了密码** |
| 安全扫描 | 默认开启 | 无 |
| 上传协议 | 强调原生协议 | npm components API + maven2 multipart，正确 |
| 增量 | 未提及 | `--skip-existing`（但每包一次 GET） |
| 报告 | 未提及 | 进度条 + 汇总 |

**本质差异**：dep-porter 解决"两端不通网"，代价是一次只搬一个坐标、依赖 Docker；
rnu 解决"这台机器有依赖、也能连上私库"，一次搬一片。你的场景（上传端同时具备公网和私网）
**不需要两段式**——dep-porter 的分阶段对你反而是纯开销。该学的是它的**命令面统一**，不是它的架构。

## 3. rnu 命令行"复杂"的实测来源

1. **全局参数必须写在子命令前面**（clap 默认行为，未加 `global = true`）：
   `nexus-upload npm ./x -s URL` 直接报 `error: unexpected argument '-s'`。
   而人的书写习惯恰恰是先敲子命令。
2. **连接信息无法沉淀**：密码只能走 `-p`（配置文件结构里没有 password 字段、没有环境变量），
   所以每条命令永远拖着 `-s -u -p -r` 四件套。
3. **maven 子命令 10 个选项、3 个互斥来源**（`<PATH>` / `--from-repo` / `--project`），
   且 `--help` 顶部说明被 clap 压成一行，不可读（实测）。
4. **npm / maven 各自重复声明同一组 5 个上传选项**（skip-existing / only / exclude / concurrency），
   选项总量去重后 20 个，分散在 3 层。
5. **repository 默认值是错的**：不传 `-r` 时兜底 `npm-hosted`，对 maven 同样生效——
   `nexus-upload maven ./target` 会静默把 Maven 构件指到 npm 仓库（应按 kind 给默认或直接报错）。

典型命令长度对比：

```
# 现在：8 个参数、135 字符，且 -s 位置写错就报错
nexus-upload -s http://nexus:8081 -u admin -p *** -r maven-releases \
  maven --project ./my-app --with-project --skip-existing -c 8

# dep-porter：短，但一次只有一个包，项目级要循环 N 次
dep-porter download --kind maven --name g:a --version 1.0
```

## 4. 简化方案（CLI v2）

目标：**日常命令只剩 `nexus-upload <kind> <path>`**，不删任何能力，只挪位置。

### 4.1 五条改动（按收益排序）

| # | 改动 | 消灭的问题 | 成本 |
|---|---|---|---|
| 1 | 所有连接/全局选项加 `global = true` | 参数位置报错；npm/maven 重复声明 | 极低（加属性） |
| 2 | 认证走环境变量 `RNU_SERVER/RNU_USER/RNU_PASSWORD/RNU_REPO`，优先级 CLI > env > profile > config | 每次拖 4 件套 | 低（ConfigResolver 加一层） |
| 3 | `config init` 交互式收集 server/user/password 并落盘（chmod 600） | 首次配置成本 | 低 |
| 4 | maven 三来源合并为一个位置参数 + 自动识别：含 `pom.xml` → project；路径呈 `g/a/v/a-v.pom` → repo 布局；其余 → loose。保留 `--layout` 显式覆盖，原 flag 转隐藏别名 | 3 个互斥选项 | 中（识别规则 + 测试） |
| 5 | repository 按 kind 取默认：profile 增加 `maven_repository` / `npm_repository`；缺失时报错而非兜底 `npm-hosted` | 静默传错仓库 | 低 |

改后形态：

```
# 一次性配置
nexus-upload config init          # 交互式：server / user / password

# 日常（0 个连接参数）
nexus-upload maven ./my-app           # 自动识别为项目 → mvn 解析闭包
nexus-upload maven ~/.m2/repository   # 自动识别为仓库布局
nexus-upload npm ./my-app             # node_modules

# CI / 脚本（env 注入，不再出现明文密码在 ps 里）
RNU_SERVER=... RNU_USER=... RNU_PASSWORD=... nexus-upload maven ./my-app --dry-run
```

### 4.2 可选（对齐 dep-porter 的坐标级输入）

- `maven --list deps.txt`：一行一个 `g:a:v`，按坐标批量。dep-porter 一次一个坐标；
  rnu 若加清单文件即获得同能力且可批量。**优先级低**——目录/项目两条路径已覆盖主场景。
- `export` 子命令（扫描→落目录，不连网）：仅当未来出现"两端物理隔离"需求再做。
  当前场景明确不需要。

### 4.3 明确不做

- **不学 `--kind/--name/--version` 单坐标模型**：那是两段式架构的产物；
  rnu 的价值在批量，改成逐包是倒退。
- **不学 Docker 化下载**：`maven --project` 已经复用本机 mvn，引入 Docker 纯增依赖。
- **不现在补 pypi/cargo**：等 npm/maven 两条链路的正确性债（scoped 包名、D1）清完再说。

## 5. 落地顺序

1. `global = true` + env 认证 + 按 kind 默认仓库（半天，纯挪动，兼容现有脚本）
2. `config init` 交互式
3. maven 自动识别（配识别规则的单元测试）
4. 帮助文案重写（修复 doc 注释被压成一行的可读性问题）

## 6. 实施状态（2026-10-21 已完成）

五条改动全部落地，另有几处顺带的修正：

| 项 | 状态 | 备注 |
|---|---|---|
| 1. `global = true` | ✅ | 12 个全局选项可写在子命令前后，实测 `npm . -s URL` 不再报错 |
| 2. 环境变量认证 | ✅ | clap `env` 属性；`RNU_SERVER/USER/PASSWORD/REPO/MVN`，优先级 CLI > env > profile > config |
| 3. `config init` 交互式 | ✅ | server / user / npm-repo / maven-repo / password 五项；密码用 `rpassword` 不回显，且在最后询问（避免与 stdin 混读错位） |
| 4. 布局自动识别 | ✅ | `MavenLayout::detect` + `--layout` 覆盖；原 `--from-repo` / `--project` 直接移除（这两个 flag 从未进过版本库） |
| 5. 仓库按 kind 解析 | ✅ | `repositories.<kind>` 为主；maven 未配置即报错，不再兜底 `npm-hosted` |

顺带修掉的：

- `#[command(version)]` 取代硬编码版本号 → 版本单一来源，`make tag` 不再需要 sed `src/cli.rs`
- `config show` 密码打码；配置文件写入后 chmod 600
- `config add` 复用全局 `-s/-u/-p`，消除了局部 `--server` 与全局参数同名遮蔽的隐患
- 错误只打印一次（原先 tracing + eprintln 各打一遍）
- `--include-dev` 标为无效并给警告（它从来没起作用）
- 帮助文案重写，maven 子命令补 `after_help` 的识别规则说明

实测：33 个单测通过；四种输入形态（project / repo / loose / pom.xml 文件）在 `--dry-run` 下识别与上传计划均正确。

