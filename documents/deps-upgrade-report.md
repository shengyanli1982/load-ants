# 依赖升级报告（load-ants）

> 说明：本报告聚焦 `Cargo.toml` 的**直接依赖**（包含 dev / target-specific）。传递依赖仅在影响升级决策时提及。

---

date: 2026-03-08
rustc: 1.93.1 (01f6ddf75 2026-02-11)
cargo: 1.93.1 (083ac5135 2025-12-15)
scope: direct-dependencies

---

## 1. 项目依赖现状（事实快照）

### 1.1 直接依赖（从 `Cargo.toml`）

| crate                     |                                Cargo.toml 版本约束 | Cargo.lock 解析版本 | 用途（基于代码引用）                                                                       |
| ------------------------- | -------------------------------------------------: | ------------------: | ------------------------------------------------------------------------------------------ |
| `hickory-server`          |             `0.24`（features: `hickory-resolver`） |            `0.24.4` | DNS 服务端/请求处理（`src/server.rs`，测试大量覆盖）                                       |
| `hickory-proto`           |                                             `0.24` |            `0.24.4` | DNS 消息/序列化（`src/server.rs`、`src/router.rs`、多处测试）                              |
| `reqwest`                 | `0.12`（no default-features；`json`+`native-tls`） |           `0.12.28` | DoH HTTP 客户端 + 自定义解析器（`src/upstream/*`、`src/config/*`）                         |
| `reqwest-middleware`      |                                              `0.4` |             `0.4.2` | HTTP 客户端中间件（重试/链路）（`src/upstream/http_client.rs` 等）                         |
| `reqwest-retry`           |                                              `0.7` |             `0.7.0` | 重试中间件（指数退避等）                                                                   |
| `retry-policies`          |                                              `0.4` |             `0.4.0` | 重试策略（被 `reqwest-retry` 使用）                                                        |
| `reqwest-lb`              |                                              `0.3` |             `0.3.1` | 上游负载均衡（`src/upstream/manager.rs`）                                                  |
| `serde`                   |                                    `1.0`（derive） |           `1.0.228` | 配置/JSON 序列化基础（`src/config/*`、`src/doh/*`）                                        |
| `serde_yaml`              |                                              `0.9` | `0.9.34+deprecated` | YAML 配置加载（`Config::from_file()` 路径）                                                |
| `serde_json`              |                                              `1.0` |           `1.0.149` | 管理 API 返回、上游 JSON 解析                                                              |
| `bytes`                   |                                             `1.10` |            `1.11.1` | HTTP body / buffer（与 `hyper`/`axum` 协同）                                               |
| `clap`                    |                                    `4.5`（derive） |            `4.5.60` | CLI 参数解析（`src/main.rs`）                                                              |
| `moka`                    |                        `0.12`（feature: `future`） |           `0.12.13` | 异步缓存（`src/cache.rs`）                                                                 |
| `tokio`                   |                         `1.44`（features: `full`） |            `1.49.0` | 异步运行时/网络 IO（`src/main.rs`、`src/server.rs`、多处测试）                             |
| `tokio-graceful-shutdown` |                                             `0.15` |            `0.15.4` | 优雅关闭与子系统管理（`src/main.rs`、`src/server.rs`、`src/admin.rs`）                     |
| `tokio-util`              |                             `0.7`（feature: `io`） |            `0.7.18` | tokio 工具库（IO 工具等）                                                                  |
| `native-tls`              |                                              `0.2` |            `0.2.18` | TLS（reqwest native-tls 后端）                                                             |
| `tracing`                 |                                              `0.1` |            `0.1.44` | 日志/追踪（核心路径多处）                                                                  |
| `tracing-subscriber`      |                                `0.3`（env-filter） |            `0.3.22` | 日志订阅器初始化（`src/main.rs`）                                                          |
| `thiserror`               |                                              `1.0` |            `1.0.69` | 错误枚举（`src/error.rs`）                                                                 |
| `anyhow`                  |                                              `1.0` |           `1.0.102` | 通用错误（当前未发现代码直接引用，可能为历史遗留或为后续扩展预留）                         |
| `mimalloc`                |                       `0.1`（no default-features） |            `0.1.48` | 可选分配器（平台/构建相关）                                                                |
| `regex`                   |                                  `1.10`（unicode） |            `1.12.3` | 规则/匹配（`src/config/rule.rs` 等）                                                       |
| `async-trait`             |                                              `0.1` |            `0.1.89` | async trait（抽象接口层）                                                                  |
| `base64`                  |                                             `0.21` |            `0.21.7` | DoH `dns=` 参数 base64url 编解码（`src/doh/handlers.rs`、`src/upstream/doh.rs`、测试覆盖） |
| `chrono`                  |                                              `0.4` |            `0.4.44` | 时间库（当前未发现代码直接引用，可能为历史遗留或为后续扩展预留）                           |
| `rand`                    |                                              `0.8` |             `0.8.5` | 随机（负载均衡/退避等）                                                                    |
| `once_cell`               |                                             `1.19` |            `1.21.3` | 全局惰性初始化（例如默认 URL）                                                             |
| `dashmap`                 |                                              `5.5` |             `5.5.3` | 并发 map（缓存/状态）                                                                      |
| `futures-util`            |                                              `0.3` |            `0.3.32` | Future 工具（stream/compat 等）                                                            |
| `axum`                    |                                              `0.8` |             `0.8.8` | 管理 API / DoH HTTP Server（`src/admin.rs`、`src/doh/*`、多处测试）                        |
| `hyper`                   |                                              `1.0` |             `1.8.1` | HTTP 组件（与 axum body/Bytes 配合，测试中使用）                                           |
| `prometheus`              |                                             `0.13` |            `0.13.4` | 指标采集与导出（`src/metrics.rs`）                                                         |
| `url`                     |                                              `2.4` |             `2.5.8` | URL 解析/构造（与 reqwest::Url 一起使用）                                                  |
| `lazy_static`             |                                              `1.5` |             `1.5.0` | 静态初始化（项目中也使用 `once_cell`）                                                     |
| `validator`               |                                   `0.19`（derive） |            `0.19.0` | 配置校验（`src/config/*`）                                                                 |

### 1.2 平台相关依赖（从 `Cargo.toml`）

| crate         | 目标平台 | Cargo.toml 版本约束 | Cargo.lock 解析版本 | 用途                                            |
| ------------- | -------- | ------------------: | ------------------: | ----------------------------------------------- |
| `openssl-sys` | unix     |   `0.9`（vendored） |           `0.9.111` | 兼容/构建 openssl（偏“构建稳定性”而非业务功能） |
| `openssl-sys` | windows  |               `0.9` |           `0.9.111` | 同上                                            |

### 1.3 dev-dependencies（从 `Cargo.toml`）

| crate            | Cargo.toml 版本约束 | Cargo.lock 解析版本 | 用途                           |
| ---------------- | ------------------: | ------------------: | ------------------------------ |
| `tempfile`       |               `3.8` |            `3.26.0` | 测试临时文件/目录              |
| `tokio-test`     |               `0.4` |             `0.4.5` | tokio 测试工具（时间控制等）   |
| `assert_matches` |               `1.5` |             `1.5.0` | 断言宏                         |
| `wiremock`       |               `0.6` |             `0.6.5` | HTTP mock（测试 DoH/上游交互） |

## 2. 下一步：查询“最新稳定版本”和升级影响

> 本节的“最新稳定版本”以 crates.io 的版本列表为准（查询日期：2026-03-08），并**忽略带 `-alpha/-beta/-rc` 的预发布版本**。

### 2.1 最新稳定版本对比（crates.io）

| crate                     | 当前解析版本（Cargo.lock） | 最新稳定版本 | 升级风险（粗分级） | 备注                                                                     |
| ------------------------- | -------------------------: | -----------: | ------------------ | ------------------------------------------------------------------------ |
| `base64`                  |                   `0.21.7` |     `0.22.1` | 高                 | `0.x` 的 minor 升级可能包含破坏性变更；需以编译错误+测试验证为准         |
| `dashmap`                 |                    `5.5.3` |      `6.1.0` | 高                 | 发生主版本变更（5→6），预计需要代码适配                                  |
| `hickory-proto`           |                   `0.24.4` |     `0.25.2` | 高                 | `0.x` 的 minor 升级，且属于 DNS 核心依赖，影响面大（大量测试覆盖是优势） |
| `hickory-server`          |                   `0.24.4` |     `0.25.2` | 高                 | 同上                                                                     |
| `moka`                    |                  `0.12.13` |    `0.12.14` | 中                 | 同 minor（`0.12.x`），理论上改动较小，但仍需验证                         |
| `prometheus`              |                   `0.13.4` |     `0.14.0` | 高                 | `0.x` minor 升级，指标导出/类型可能有调整                                |
| `rand`                    |                    `0.8.5` |     `0.10.0` | 高                 | `0.x` 跨多个 minor 版本，常见 API 变化点较多                             |
| `reqwest`                 |                  `0.12.28` |     `0.13.2` | 高                 | `0.x` minor 升级；且与 TLS/代理/自定义解析器相关，需重点回归             |
| `reqwest-middleware`      |                    `0.4.2` |      `0.5.1` | 高                 | `0.x` minor 升级；与 `reqwest` 组合需联动升级                            |
| `reqwest-retry`           |                    `0.7.0` |      `0.9.1` | 高                 | `0.x` 跨多个 minor；与中间件 API 联动                                    |
| `retry-policies`          |                    `0.4.0` |      `0.5.1` | 高                 | `0.x` minor 升级；通常与 retry 生态一起升级                              |
| `thiserror`               |                   `1.0.69` |     `2.0.18` | 高                 | 主版本变更（1→2），需要验证 `#[derive(Error)]` 相关用法兼容性            |
| `tokio`                   |                   `1.49.0` |     `1.50.0` | 低                 | 同主版本，通常兼容；但可能带来 MSRV/行为细节变化                         |
| `tokio-graceful-shutdown` |                   `0.15.4` |     `0.19.2` | 高                 | `0.x` 跨多个 minor；与子系统/关闭语义紧密相关，需要集成测试回归          |
| `validator`               |                   `0.19.0` |     `0.20.0` | 高                 | `0.x` minor 升级；derive/错误结构可能变化，影响配置校验逻辑              |

### 2.2 初步升级策略（先风险控制，再追求“最新”）

建议按“依赖耦合度 + 影响面”拆成 3 组，小步推进并保持可回滚：

1. **低风险快速收益（先做）**
    - `moka 0.12.13 → 0.12.14`、`tokio 1.49.0 → 1.50.0`（以及其它已是最新的依赖仅做 `Cargo.lock` 更新）
2. **生态联动升级（一起做更稳）**
    - `reqwest` + `reqwest-middleware` + `reqwest-retry` + `retry-policies`（必要时包含 `reqwest-lb`，但它当前已是最新）
3. **核心协议/运行时语义（单独做，强回归）**
    - DNS：`hickory-*`
    - 管理与稳定性：`tokio-graceful-shutdown`、`validator`、`prometheus`
    - 错误系统：`thiserror`
    - 数据结构：`dashmap`

> 下一阶段会把上述“风险分级”落到可执行计划：逐个升级、编译/测试验证、记录适配点，并把最终结论回填到本报告。

## 3. 新版本特性研究（Context7 + DeepWiki + 项目代码对照）

> 本节用于满足“先了解新版本特性、再制定计划”的门禁：先从官方文档/仓库信息提取变更点，再结合本项目代码使用点评估兼容性与潜在优化。

### 3.1 `reqwest 0.12 → 0.13`（与中间件生态联动）

#### 新特性/关键变化（整理自 DeepWiki + 文档）

- feature 调整：`query` / `form` 不再默认启用（需要显式打开）。
- TLS 相关：TLS 后端与 feature/方法命名有调整；对 `rustls` / `native-tls` 的选择更明确。
- `ClientBuilder`：新增/重命名若干 TLS 配置方法以提升可发现性（旧方法可能软弃用）。
- DNS 扩展点：提供更明确的自定义 resolver 扩展点（例如 `dns_resolver2` 一类接口；细节以最终编译与测试为准）。

#### 与本项目的兼容性对照（dev-repo-refactor）

- 代码使用点：
  - `src/upstream/http_client.rs`：构建 `reqwest::ClientBuilder`，配置 `Proxy`，并与 `reqwest-middleware`/`reqwest-retry` 组合。
  - `src/upstream/bootstrap_dns.rs`：使用 `reqwest::dns::{Addrs, Name, Resolve, Resolving}` 自定义解析器（docs.rs 仍存在 `reqwest::dns` 模块）。
  - `src/config/*`：使用 `reqwest::Url` 解析/校验配置。
- 预计影响：
  - 需要**联动升级**：`reqwest` + `reqwest-middleware` + `reqwest-retry` + `retry-policies`，否则可能出现依赖树不兼容。
  - 若项目后续需要更强的上游解析控制（例如 bootstrap DNS 缓存/策略），0.13 的 DNS 扩展点更利于架构演进。

### 3.2 `hickory-* 0.24 → 0.25`（DNS 核心依赖）

#### 新特性/关键变化（整理自 release notes + DeepWiki）

- 0.25.0 被标注为“较大版本”，包含多项 breaking changes。
- 安全与协议细节：对 UDP 客户端响应的验证更严格（源地址/ID/查询名校验等），降低投毒风险的概率。
- 传输支持：引入/增强对加密/新传输的支持（例如 DoH3/HTTP3；是否能被本项目利用取决于我们的监听/上游设计）。
- DNS name 处理：`Name` 默认带“root label”（需要留意测试里对域名字符串的期望）。
- TLS 支持策略：release notes 提到移除某些 TLS 依赖/特性（本项目当前未启用 hickory 的 TLS 特性，但需要确认 feature 名是否变化）。

#### 与本项目的兼容性对照（dev-repo-refactor）

- 代码使用点：
  - `src/server.rs`：`hickory_server::ServerFuture`、`RequestHandler`、`MessageResponseBuilder`、`Protocol`、以及 `hickory_proto` 的 `Message/Query/Name` 与二进制编解码。
  - 大量 integration tests 覆盖 DNS message 构造与服务器适配（这是升级的护栏）。
- 预计影响：
  - 可能出现类型/API 微调导致编译错误（需逐处修复）。
  - 行为变化（更严格校验、`Name` rooting）需要用现有测试验证，必要时更新测试期望或处理逻辑。

### 3.3 `tokio-graceful-shutdown 0.15 → 0.19`（优雅关闭/子系统编排）

#### 新特性/关键变化（整理自 docs.rs 示例与 API）

- `Toplevel::new(...)` 在新版本示例中使用 `async |s: &mut SubsystemHandle| { ... }` 的形式（句柄以可变引用传入）。
- `handle_shutdown_requests(...)` 的返回类型变为 `Result<(), GracefulShutdownError<ErrType>>`（错误类型更结构化）。

#### 与本项目的兼容性对照（dev-repo-refactor）

- 代码使用点：
  - `src/main.rs`：`Toplevel::new(|s| async move { ... s.start(...) ... })`（需要迁移到新签名/用法）。
  - `src/admin.rs` / `src/server.rs`：通过 `IntoSubsystem<AppError>` 实现与 `SubsystemHandle` 协作。
- 预计影响：
  - 这是**确定需要代码适配**的一组升级（签名变化明确）。
  - 升级收益在于：更清晰的 shutdown 语义与错误回传（更利于生产可观测性与可靠退出）。

### 3.4 `prometheus 0.13 → 0.14`（指标）

#### 新特性/关键变化（整理自 DeepWiki）

- 标签值 API：owned label values 改用 `AsRef<str>`（更灵活，可能有性能收益）。
- 依赖与 MSRV：0.14.0 将 MSRV 提升到 Rust 1.81（本机 rustc 1.93.1 满足，但需要确认生产环境）。

#### 与本项目的兼容性对照（dev-repo-refactor）

- 代码使用点：`src/metrics.rs`（大量 `HistogramVec`/`IntCounterVec`/`Registry`、encoder 导出）。
- 预计影响：大概率是“编译即可过”的升级；若有 label 值类型/方法弃用，则按编译报错小修。

### 3.5 `validator 0.19 → 0.20`（配置校验）

- 新特性：`ValidationErrors` 支持 `Deserialize`；修复嵌套结构中 `custom` 验证器执行时机的 bug（DeepWiki）。
- 本项目影响面：`src/config/*` 与 `tests/config_tests.rs`（主要是错误格式化与错误树遍历）。
- 预期：以测试验证为准，可能是“无痛升级或小改”。

### 3.6 `rand 0.8 → 0.10`（随机/打散）

- 变更幅度较大：`thread_rng()` 更名为 `rng()`；`Rng`/`RngExt` 拆分；部分方法与模块重命名（DeepWiki）。
- 本项目使用点：
  - `src/balancer.rs`：随机选择上游（`IndexedRandom::choose`）。
  - `src/cache.rs`：对 A/AAAA 记录随机打散（`SliceRandom::shuffle`）。
- 结论：升级带来的架构收益有限，但代码改动也很局部（可作为“后置可选项”，或在最后集中处理）。

### 3.7 `serde_yaml` 现状与替代（重要：维护性风险）

- 现状：`serde_yaml 0.9.34+deprecated` 已被标注 deprecated 且不再维护（DeepWiki）。
- 替代候选：`serde_yaml_ng`（fork，目标是持续维护并尽可能保持 API 兼容；DeepWiki）。
- 本项目使用点：`Config::from_file()` 的 `serde_yaml::from_str` 反序列化（以及可能的序列化）。
- 结论：从“生产级/长期维护”角度，这是一个**优先级很高**的升级/迁移项，即使它不是简单的“升版本号”。

## 4. 优先级建议（以“架构收益 + 风险”综合排序）

> 该优先级用于指导“先做能带来能力/维护性提升的升级”。最终以你确认的升级策略为准。

1. P0：`serde_yaml → serde_yaml_ng`（消除 deprecated/无人维护风险；迁移成本预计较低）
2. P1：`reqwest` 生态联动升级（客户端能力/特性演进 + 依赖一致性）
3. P1：`hickory-*` 升级（DNS 核心 + 安全/协议细节改进）
4. P1：`tokio-graceful-shutdown` 升级（可靠 shutdown 语义与错误结构化；但需要代码适配）
5. P2：`prometheus` / `validator` / `base64` / `dashmap`（按编译与测试驱动小步推进）
6. P3：`rand`（变更大但收益相对有限；建议最后做，避免干扰核心链路排障）

## 5. 已执行升级结果（落地记录）

> 时间：2026-03-08（本次变更已完成编译与回归验证）

### 5.1 直接依赖最终解析版本（`cargo tree --depth 1`）

| crate                     | 解析版本 |
| ------------------------- | -------: |
| `hickory-proto`           | `0.25.2` |
| `hickory-server`          | `0.25.2` |
| `reqwest`                 | `0.13.2` |
| `reqwest-middleware`      | `0.5.1`  |
| `reqwest-retry`           | `0.9.1`  |
| `retry-policies`          | `0.5.1`  |
| `tokio-graceful-shutdown` | `0.19.2` |
| `tokio`                   | `1.50.0` |
| `moka`                    | `0.12.14` |
| `base64`                  | `0.22.1` |
| `dashmap`                 | `6.1.0`  |
| `prometheus`              | `0.14.0` |
| `validator`               | `0.20.0` |
| `thiserror`               | `2.0.18` |
| `rand`                    | `0.10.0` |
| `serde_yaml`              | `0.9.34+deprecated`（保持不变） |

### 5.2 已验证命令（通过）

- `cargo test`
- `cargo run -- -c .\\config.default.yaml --test`
