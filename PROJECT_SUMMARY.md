# RNU - Rust Nexus Uploader 项目总结

## 项目概述

RNU (Rust Nexus Uploader) 是一个基于 Rust 开发的 CLI 工具，支持将 npm 包和 Maven 构件批量上传到 Nexus Repository Manager 3.x。适用于离线环境、企业内网、CI/CD 流水线等场景。

二进制名：`nexus-upload`

## 当前实现 (v0.2.0)

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

## 未来规划

- [ ] PyPI 包支持
- [ ] `--include-dev` 实现（npm devDependencies 过滤）
- [ ] 上传失败重试
- [ ] 配置文件 `config remove` 子命令

## License

Apache-2.0