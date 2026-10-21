# rnu 功能完善路线图

> 基线：`v0.2.0`（2458 行源码，15 个单测通过 / 2 个 ignored）
> 打包链路已可跨平台出包，以下只讨论**功能与正确性**。

---

## 一、已实证的缺陷

下列四条不是"读代码觉得有问题"，而是在本仓库内临时加探针测试跑出来的实际输出（探针已移除，仓库保持干净）。

> **状态**：D3 / D4 已修复（`split_artifact_filename` 强制版本边界并解析 classifier；`--from-repo` / `--project` 路径从设计上不经过 XML 解析）。D1 / D2 待修。

### D1 Maven 坐标解析会错乱 —— 最严重

任意 `<groupId>` / `<version>` 出现的位置都会覆盖项目级坐标，因为 `parse_pom()` 只记"上一个开始标签"，没有层级概念。

```xml
<project>
  <groupId>com.example</groupId>
  <artifactId>foo</artifactId>
  <version>1.2.3</version>
  <dependencies>
    <dependency>
      <groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>2.0.9</version>
    </dependency>
  </dependencies>
</project>
```

实测解析结果：

```
解析结果 => org.slf4j:slf4j-api:2.0.9     ← 期望 com.example:foo:1.2.3
```

影响：上传路径、`--skip-existing` 查询、`--only` 过滤全部基于错误坐标。含 `<dependencies>` / `<build><plugins>` / `<profiles>` / `<distributionManagement>` 的 pom 几乎全部命中——也就是**真实世界的 pom 基本都错**。

改法：维护元素路径栈，只接受 `<project>` 直接子节点的坐标；`<parent>` 单独处理（仅用于继承默认值，且不得反写 artifactId）。

### D2 scoped 包名被截断

`extract_name()` 先命中 `rfind('/')`，`@types/node` 变成 `node`。实测：

```
extract_name("@types/node") => "node"
```

连锁后果（均已验证）：

| 场景 | 表现 |
|---|---|
| `--only "@types/node"` | 匹配不上（`"node".contains("@types/node")` 为 false），一个都不传 |
| `--exclude "@types/node"` | 排除不掉 |
| 去重 | `@a/react` 与 `@b/react` 同版本被合并成 1 个，实测扫到 2 个目录只输出 1 个包 |
| `--skip-existing` | 查的是 `name=node`，查不到 `@types/node`；若仓库里真有个 `node` 包同版本，会**误判为已存在而跳过** |

注意：上传本身是好的——Nexus 从 tgz 内的 `package.json` 读真实包名。所以这是个"只在过滤/去重/查重上出错"的隐蔽 bug。

改法：`PackageEntry.name` 保留完整名（`@types/node`），另加 `base_name`（`node`）只用于 tarball 文件名与 npm 路径拼接。

### D3 Maven 版本前缀越界

匹配同目录伴生文件时用的是 `starts_with("{artifact}-{version}")`，没有版本边界。实测 `foo:1.0` 收集到：

```
["foo-1.0.pom", "foo-1.0.jar", "foo-1.0.1.jar"]   ← 混进了 1.0.1 的 jar
```

改法：要求 `{artifact}-{version}.` 或 `{artifact}-{version}-` 前缀。

### D4 sources / javadoc jar 缺 classifier

`classify_extension()` 把 `-sources.jar` / `-javadoc.jar` 一律归一成 `jar`，而上传时只发 `maven2.assetN.extension`，从不发 `maven2.assetN.classifier`。Nexus 无法区分，sources 包会被当作主构件写入（同一 component 内后者覆盖前者）。

改法：解析 `{artifact}-{version}-{classifier}.{ext}`，classifier 一并提交。

---

## 二、可靠性缺口

| 编号 | 问题 | 现状 | 建议 |
|---|---|---|---|
| R1 | 无重试 | 一次 502 / 超时即判该包失败，整批 exit 1 | 对 429 / 5xx / 连接错误做退避重试（3 次，抖动） |
| R2 | 400/409 未视为成功 | 已存在时上传返回 400，被计为 error | 409 及响应体含 "already exists" 的 400 视为 skipped |
| R3 | 鉴权错误被吞 | `check_package_exists` 用 `.unwrap_or(false)`，401 被当成"不存在" | 鉴权错误立即中断，而不是继续上传 N 个再全红 |
| R4 | 无预检 | `test_connection()` 存在但挂 `#[allow(dead_code)]` | 批量前调一次，配错时 1 条明确报错而不是 N 条 |
| R5 | 部分上传被当完整 | Maven `check_maven_exists` 只看"有没有 item" | 按文件（含 classifier）逐个判定 |
| R6 | 跳过数不计入统计 | `increment_skipped` 定义了但全仓无调用点 | `filter_existing` 把 skipped 回传，summary 补上 |
| R7 | 整文件读入内存 | `read_to_end` / `fs::read` 后 `Part::bytes` | 改用流式 `Part::stream`，避免 4 并发 × 大 jar OOM |
| R8 | 临时包名冲突 | `temp_dir().join("{name}-{version}.tgz")` | 用 `tempfile::NamedTempFile`，两进程并行不互相踩 |
| R9 | 清理失败覆盖真实错误 | `fs::remove_file(...)?; result` 顺序反了 | 先取 result，清理失败只 warn |

---

## 三、性能与交互

**P1 `--skip-existing` 的请求量是 O(n)**

当前每个包一次 `GET /service/rest/v1/search`。2000 个包 = 2000 次请求，并发 4。跨机房时这是分钟级开销。

建议分页拉一次全集：`GET /service/rest/v1/search?repository=X` 是 100 条/页 + `continuationToken`，构建 `HashSet<(name, version)>` 后本地判定。请求数从 O(n) 降到 O(n/100)。

**P2 输出不可机读**

- 日志写 stdout（`init_tracing` 的 `with_writer(std::io::stdout)`），污染管道
- 非 TTY 下 indicatif 仍输出控制字符，CI 日志变乱码
- 没有 `--json` 结果报告，没有"失败清单文件"
- 退出码恒为 1，无法区分"部分失败"与"配置错误"

建议：日志走 stderr；`is_terminal()` 为 false 时降级为逐行输出；新增 `--report <path>` 输出 JSON（每项 name/version/status/error）与 `--json` 到 stdout。

**P3 进度条与并发数**

默认 `-c 4` 偏保守。小包场景可参考 CPU 数或按包体大小自适应；大 jar 场景则应限制并发以防内存峰值。

---

## 四、功能补齐

| 功能 | 状态 | 说明 |
|---|---|---|
| `--include-dev` | **假开关** | `run()` 里是 `_include_dev`，从未使用；而 scanner 本来就扫全部 `node_modules`，传与不传毫无区别。要么按 `package-lock.json` 的 `dev: true` 实现，要么删掉这个参数 |
| `--only` / `--exclude` | 子串匹配 | `--exclude test` 会误伤 `latest-package`。建议改为精确名或 glob（`--only "@types/*"`） |
| `config remove` | 缺 | 有 add 没有 remove / set-default，profile 只能手工改 JSON |
| `pypi` | 隐藏空壳 | `#[command(hide = true)]`，只打印"未实现"。Nexus pypi hosted 支持 `POST /repository/pypi-hosted/`（`:action=file_upload` + md5/sha256/metadata），可从 wheel 文件名与 `METADATA` 提取字段，工作量中等 |
| 仓库类型校验 | 缺 | npm 包传进 maven 仓库只得到 400，报错信息无指向性。可在启动时查 `/service/rest/v1/repositories` 校验 `format` 与子命令是否匹配 |
| tar 打包规则 | 简化 | 所有文件统一 `0o755`（应为 `0o644`）；不读 `.npmignore` / `files` 字段；不处理符号链接 |

---

## 五、测试与门禁

- `tests/nexus_test.rs` 只有 4 行，等于没有集成测试
- 无 HTTP mock，`client.rs` 的 401/403/400/409/分页/重试分支全部未被覆盖
- `make check`（`clippy -D warnings`）当前必然失败：**25 条告警**，其中 `src/utils/mod.rs` 整个模块（`format_bytes` / `confirm_prompt` / `print_*` / `truncate_string` / `is_valid_url` / `sanitize_package_name`）与 `SimpleProgress` 零引用
- CI 只跑 `fmt --check` + `test`，没有 clippy

建议顺序：先删死代码让 clippy 归零 → 纳入 CI 门禁 → 引入 `wiremock`（或 `httpmock`）补客户端分支测试 → 把 D1~D4 的探针改写成常驻回归测试。

**D1~D4 的回归测试骨架**（本轮验证时用过，可直接落库）：

```rust
// maven.rs
#[test]
fn pom_coordinates_survive_dependencies() { /* 期望 com.example:foo:1.2.3 */ }

#[test]
fn version_prefix_has_boundary() { /* foo:1.0 不收 foo-1.0.1.jar */ }

#[test]
fn classifier_is_preserved() { /* foo-1.0-sources.jar → classifier="sources" */ }

// scanner.rs
#[test]
fn scoped_name_keeps_scope() { /* "@types/node" 原样保留 */ }

#[test]
fn scoped_packages_do_not_collide() { /* @a/react 与 @b/react 各自保留 */ }
```

---

## 六、安全

**S1 已泄露的凭据（最高优先）**

`src/nexus/mod.rs` 的 `test_config()` 里硬编码了内网地址、`admin` 与明文密码；`test_upload_tarball()` 硬编码了本机绝对路径。仓库是公开的且已推送到 GitHub，**这些内容已存在于 git 历史**。

处理顺序：
1. 立即轮换该账号密码
2. 测试改为读环境变量（`nexus_test_config()` 从 `RNU_TEST_*` 读取，缺失则 `return` 跳过）
3. 视需要清理历史（`git filter-repo`）——仅当该密码确实敏感时

**S2 密码只能走命令行**

`build_config()` 强制从 `cli.password` 取值，没有环境变量也没有凭据文件。CI 里 `-p xxx` 会出现在进程列表与日志中。

建议：`RNU_PASSWORD` / `NEXUS_PASSWORD` 环境变量优先于 `--password`，并支持 `--password-file -` 从 stdin 读。配置文件继续不存密码（目前是对的）。

---

## 七、建议排期

| 阶段 | 内容 | 理由 |
|---|---|---|
| **一 · 正确性** | D1 POM 坐标、D2 scoped 包、D3 版本边界、D4 classifier + 5 条回归测试 | 这四条会让工具"看起来成功但写错仓库"，优先于任何新功能 |
| **二 · 安全** | S1 轮换凭据 + 测试改环境变量、S2 环境变量取密码 | S1 已在公开仓库，越晚成本越高 |
| **三 · 可靠性** | R1~R9（重试、预检、跳过统计、流式上传、临时文件） | 决定能否在真实弱网/大包场景下跑完 |
| **四 · 效率与 CI** | skip-existing 分页、日志走 stderr、`--report json`、clippy 归零并入 CI | 批量场景与流水线集成的实际门槛 |
| **五 · 功能** | glob 过滤、`--include-dev` 落地或下线、`config remove`、pypi | 增量价值，可独立发布 |

---

## 八、Maven 待上传清单的三种获取方式

问题：能不能用 `mvn dependency:list` / `dependency:tree` 代替现在"递归扫目录找 pom"？

**结论：`dependency:list` 解决的是"这个项目需要什么"，当前方案解决的是"这个目录里有什么"。前者语义更好，但代价是把工具的离线前提反转了；真正该做的是第三种——扫本地仓库布局。**

### 8.1 对比

| 维度 | 当前：扫目录找 `.pom` | `mvn dependency:list` | 扫 Maven 仓库布局 |
|---|---|---|---|
| 坐标来源 | 自己解析 XML | Maven 权威解析 | **路径**（`g/a/v/a-v.jar`） |
| 已知缺陷 | D1 坐标错乱、D3 版本边界、D4 classifier | 无（不解析 XML） | **三者从根上不存在** |
| 语义 | 目录里有什么就传什么 | 该项目的依赖闭包（含传递依赖） | 仓库里有什么就传什么 |
| 能否覆盖本项目自身产物 | 能 | **不能**（reactor 模块不在依赖列表里） | 能（`mvn install` 后就在仓库里） |
| parent pom | 能（目录里有就传） | **默认不包含**（`includeParents` 默认 `false`） | 能 |
| sources / javadoc | 能（但缺 classifier） | 不能（需另跑 `dependency:sources`） | 能 |
| 外部依赖 | 无，纯文件系统 | **JDK + Maven + 可解析的 pom + 可达仓库** | 无 |
| 离线场景 | 天然支持 | **直接 BUILD FAILURE**（这正是 rnu 的存在前提） | 天然支持 |
| 单次耗时 | 几百毫秒（纯遍历） | 5–30 秒（JVM 启动 + 模型解析 + 可能联网） | 秒级 |
| 分发形态 | 单个静态二进制 | 需在 PATH 找 `mvn` / `mvn.cmd`，处理 JAVA_HOME | 单个静态二进制 |
| 失败模式 | 静默错（最危险） | 一堵 `BUILD FAILURE` 日志，需判退出码 | 静默错可能性低 |

### 8.2 mvn 方案的四个隐性成本

1. **前提反转**：`dependency:list` 要求依赖能被解析。在真正离线的内网机器上跑，得到的是 `Could not resolve dependencies` 而不是清单。所以 mvn 只能放在**有网的源端**，等于把一次操作拆成"源端生成清单 → 搬运 → 目标端上传"三段。
2. **漏掉自身产物**：聚合工程上跑 `dependency:list`，只会列出第三方依赖，`com.example:foo:1.2.3` 这类本项目模块不在其中。要补齐得先 `mvn install`——而一旦 install，这些文件就进了本地仓库，那不如直接扫仓库。
3. **parent pom 缺失**：`includeParents` 默认 `false`。只上传清单里的构件，内网构建时会因为找不到 parent 而失败。
4. **清单只是坐标，还得回本地仓库取文件**：`outputAbsoluteArtifactFilename=true` 能带上绝对路径（官方参数，默认 `false`），但这已经说明——文件本来就在仓库里按布局放着。

另外：`dependency:tree` 是给人看的（缩进树 + 冲突裁决），机器解析脆弱；要机器读就用 `dependency:list -DoutputFile=...`，格式稳定为 `groupId:artifactId:type[:classifier]:version:scope`。参数依据官方 `maven-dependency-plugin`（3.11.0）文档，本机 mvn 因沙箱限制未能实跑，格式以上述文档为准。

### 8.3 为什么仓库布局是最优解

`~/.m2/repository/org/slf4j/slf4j-api/1.5.6/` 下的实际内容（本机抽样）：

```
_remote.repositories
slf4j-api-1.5.6.jar
slf4j-api-1.5.6.jar.sha1
slf4j-api-1.5.6.pom
slf4j-api-1.5.6.pom.sha1
```

- **坐标 = 目录路径**，不需要任何 XML 解析 → D1 消失
- **版本 = 目录名**，天然有边界 → D3 消失
- **classifier = 文件名中 `{artifact}-{version}` 之后、扩展名之前的部分** → D4 变成一次字符串切分
- 只需过滤噪声：`_remote.repositories`、`*.lastUpdated`、`*.sha1/.md5/.asc`、`maven-metadata*.xml`
- 产它的人是谁无所谓：`mvn install`、`dependency:go-offline`、`dependency:copy-dependencies -Dmdep.useRepositoryLayout=true`（官方参数，输出 `outputDirectory/g/a/v/a-v.jar`）都行，Gradle 往 `~/.m2` 装的也一样

对应的工作流正好匹配内网搬运的现实做法：

```
源端（有网）  mvn -DskipTests install         → ~/.m2/repository 齐全（含本项目模块）
搬运          tar 打包 repository 目录
目标端（离线） rnu maven --from-repo ./repository
```

### 8.4 前提变更：上传端同时具备公网与私网

上面 8.1–8.3 的一条关键约束随之消失：既然上传端能访问公网仓库，`dependency:go-offline` 就可以**在上传端直接跑**，不必拆成"源端生成清单 → 搬运 → 目标端上传"三段。此时"按项目按需"成为主场景，而"整仓搬运"退为次要场景。

但直接取 `dependency:list` 的坐标清单仍有三个漏洞：不含本项目自身产物、默认不含 parent pom、不含 sources/javadoc。更稳的做法是**让 mvn 把依赖物化成一份隔离的本地仓库，然后扫这个仓库**：

```
rnu maven --project ./my-app
  └─ mvn -q -B -Dmaven.repo.local=$TMP dependency:go-offline
       └─ 扫 $TMP（仓库布局：坐标=路径，版本=目录名，classifier=文件名）
            └─ 逐个上传到私库
```

这样解析交给 Maven（BOM import、dependencyManagement、exclusions、版本调解全部正确），而上传侧只做自己擅长的事——按路径取坐标、multipart 推包。

**收益**

- 文件集合 = "Maven 实际下载了什么"，天然包含 parent pom、BOM、插件依赖
- 不需要解析 mvn 的 stdout（locale / 颜色 / 版本差异都不影响），只看退出码
- 坐标来自路径，D1 / D3 / D4 在主力路径上不发生
- `-Dmaven.repo.local` 隔离，不污染用户 `~/.m2/repository`

**必须处理的坑**

1. **解析源与私库的关系要理清，但别急着替换用户的 settings。**
   实测本机 `~/.m2/settings.xml` 的 `custom-repos` profile 把 `private-repo`（`http://20.21.1.180:18081/repository/maven-releases/`）排在 `aliyun` **之前**，mirror 又是 `!SI_MAVEN,!zyjc-releases,!zyjc-snapshots` 这种只含排除项的写法。后果：`go-offline` 会先打私库，拿到 404 才回落公网（多一倍延迟）；若私库返回的是 401/500 而不是 404，解析直接失败。

   但**不要**用"生成临时 settings 把 mirror 指向公网"这种一刀切做法——项目很可能依赖只存在于私库的内部构件（`zyjc-*`），切断私库会让解析失败。

   正确做法：**只隔离 `-Dmaven.repo.local`，解析照用用户现有 settings**；上传阶段把临时仓库里的**全部内容**推上去，已经存在于私库的交给 `--skip-existing` 挡掉。这样根本不需要判断"这个构件来自公网还是私库"，省掉一层容易出错的来源追踪。

   `--resolve-from` 只作为排障选项保留（私库异常时强制只走公网），不是默认路径。
2. **`go-offline` 有已知漏项**：部分插件依赖与动态版本范围（`LATEST` / `RELEASE` / 区间）不会被拉到。补跑 `dependency:resolve-plugins`，并以"上传后回查"作为兜底，而不是声称 100% 完整。
3. **快照版本语义不同**：`-SNAPSHOT` 由 Nexus 侧生成时间戳目录，`--skip-existing` 不能按 version 判定，需单独走"按时间戳比较"或直接 `--force`。
4. **自身产物**：`go-offline` 不含本项目模块，要 `install -DskipTests` 才会出现在临时仓库里。用 `--with-project` 开关控制。
5. **体积与耗时**：中型微服务的依赖闭包通常几百 MB，`go-offline` 一次 30 秒到数分钟。提供 `--keep-repo <dir>` 复用作缓存，避免每次重下。

**备选的低成本形态**：`dependency:copy-dependencies -Dmdep.useRepositoryLayout=true -DoutputDirectory=<dir>` 直接产出精简的仓库布局目录，只含已解析的构件（不含 parent pom 与插件）。适合"只想要 jar"的场景，比 `go-offline` 轻，但完整性差一档。

### 8.5 命令面

| 形式 | 语义 | 定位 |
|---|---|---|
| `maven --project <dir>` | 按项目解析依赖闭包并上传 | **主力**，对应"按项目按需" |
| `maven --from-repo <path>` | 扫已有仓库布局直接上传 | 次主力，适合已有离线仓库 |
| `maven <dir>` | 扫目录里的 `.pom` | 保留，兼容松散目录（需修 D1/D3/D4） |
| `--scope runtime\|compile\|test\|provided` | 映射 `includeScope`，默认 `runtime`（**未实现**，见 8.6-7） | 控制"按需"的边界 |
| `--with-project` | 连本项目模块一起上传 | 配合 `install` |
| `--keep-repo <dir>` | 复用临时仓库，不全量重下 | 迭代调试 |
| `--resolve-from <url>` | 排障用：强制只走指定公网源，绕开有问题的私库（**未实现**） | 非常规路径 |

原则修正：**在"按项目按需"这个目标下，mvn 不再是可有可无的加速器，而是解析层的正解** —— 自己实现 Maven 的依赖调解（BOM import、版本区间、exclusions、最近优先）没有可行性。但上传层必须保持独立于 mvn：`--from-repo` 与 `maven <dir>` 两条路径不依赖 Java，保证"只有私库、没有公网也没有 JDK"的兜底场景仍然可用。

### 8.6 实施顺序

1. ✅ `MavenRepoScanner`（路径→坐标、classifier 解析、噪声过滤）
2. ✅ `upload_maven_component` 补 `maven2.assetN.classifier` —— 顺带修掉 D4
3. ✅ `--from-repo` 接线，假仓库端到端验证（含抓包确认字段）
4. ✅ `--project` 包装：临时仓库 + mvn 调用 + 失败时回显最后 25 行 + `--with-project` / `--keep-repo` / `--mvn`
5. ⬜ `maven <dir>` 路径的 D1 修复（D3 / D4 已随本次重构修掉）
6. ⬜ `--resolve-from`（排障用，暂未实现）
7. ⬜ `--scope`（`go-offline` 会解析全部 scope；若需要收敛，得改用 `copy-dependencies` + 补 pom，暂不做假开关）
