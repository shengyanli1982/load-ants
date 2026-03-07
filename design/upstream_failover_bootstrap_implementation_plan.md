# 实施方案定稿：上游请求内 Failover + Bootstrap DNS + 被动熔断（基于 spec）

> 依据：`design/upstream_failover_bootstrap_spec.md`  
> 注意：用户明确要求 **跳过** `design/upstream_failover_bootstrap_spec copy.md`，本文不引用该文件内容。  
> 仓库现状基线：vNext 配置（`config.default.yaml` / `config.example.yaml`）+ Rust 实现（见“代码入口与差距”）。

---

## 0. 目标与非目标

### 0.1 目标（Outcome）

1. **请求内 failover**：同一次 DNS 查询在预算内可在“组 → server”维度做有限次尝试，尽量成功返回。
2. **被动熔断（cooldown）**：连续失败的上游端点进入 cooldown，cooldown 内不会被选中，避免坏节点放大尾延迟。
3. **Bootstrap DNS**：为 DoH upstream（以及可选 proxy）的 hostname 解析提供可控的引导解析器，避免隐式依赖系统 resolver。

### 0.2 Non-goals（边界）

- 不实现并发竞速（parallel race）与主动健康检查（active healthcheck）。
- 不改变 `protocol=doh|dns` 的语义边界，不把 HTTP 语义配置扩展到 classic DNS 上游。
- `bootstrap_dns` **禁止**成为“通用解析接口”，只用于内部解析 upstream/proxy hostname。

---

## 1. 代码入口与差距（真相链）

### 1.1 入口

- 转发调用链：`src/handler.rs` → `RequestHandler::handle_forward()` → `UpstreamManager::forward()`
- 上游实现：`src/upstream/manager.rs` → `UpstreamManager::forward()`（当前仅单次选择 + 单次请求）
- DNS 客户端：`src/upstream/dns_client.rs` → `DnsClient::send_to()`（当前固定 UDP→若 TC=1 则 TCP 重试）
- DoH HTTP client：`src/upstream/http_client.rs` → `HttpClient::create()`（reqwest ClientBuilder，当前依赖系统解析）
- 负载均衡：`src/balancer.rs` → `LoadBalancer`（当前 `report_failure()` 在 RoundRobin/Weighted/Random 中均为 no-op）
- 配置 Schema：`src/config/*`（全局 `deny_unknown_fields`，新增字段必须入 Schema；已有 `tests/config_tests.rs` 兜底）

### 1.2 当前与 spec 的主要差距

1. 缺少请求内 failover（当前 `forward()` 只尝试一次）。
2. 被动熔断无效（LB 的 `report_failure()` 无状态）。
3. 缺少 server 级 `transport`（`udp|tcp`）与 `TC=1` 语义说明（期望：首发 UDP 时同端点升级 TCP，且 `TC=1` 本身不计入 health failure）。
4. 缺少 bootstrap_dns（DoH hostname/proxy hostname 解析不可控）。
5. 配置层缺少 `bootstrap_dns / fallback / failover / health / transport` 结构体与校验。

---

## 2. 关键设计决策（定稿）

> 本节决定“默认行为/兼容性/实现复杂度”，后续实现严格遵循。

### 2.1 向后兼容优先（默认不改变既有配置行为）

- 仅当用户在 vNext 配置中显式启用相关块时，才开启新增能力：
    - `upstreams[].fallback` / `upstreams[].failover` / `upstreams[].health` / 顶层 `bootstrap_dns`
- 若配置不包含这些字段：行为保持现状（不做请求内 failover；DoH hostname 走系统 resolver）。

> 解释：该策略与本仓库现有 `deny_unknown_fields` + “仓库自带配置文件必须能加载”的测试体系最稳妥；同时保留未来把“默认启用”提升为推荐写法的空间（可通过文档/示例配置推动）。

### 2.2 HTTP 非 2xx 的 failover 触发默认（保守）

- 默认仅将 **网络/超时/TLS** 与 **HTTP 5xx** 视为 transport error（可触发 failover）。
- **HTTP 429**：暂不默认触发（保留未来加开关）。
- **HTTP 4xx（非 429）**：默认不触发 failover（认为是配置/请求语义错误）。

### 2.3 Bootstrap DNS 作用域（严格）

- Bootstrap resolver **仅**注入 upstream DoH HTTP client（`src/upstream/http_client.rs`），不复用于 `remote_rules` 下载 client（避免把“引导解析”扩散成通用解析层）。

---

## 3. 配置设计（vNext Schema 落地）

> 目标：在不破坏现有 vNext 配置的前提下，引入 spec 所需结构，并提供严格校验与清晰错误信息。

### 3.1 顶层新增：`bootstrap_dns`（可选）

```yaml
bootstrap_dns:
    groups: ["public_dns"] # 必须：引用 protocol=dns 的 upstream 名称
    timeout: 2 # 秒，范围 1-30
    cache_ttl: 300 # 秒，范围 0-86400；0 禁用缓存
    prefer_ipv6: false # 是否优先 AAAA
    use_system_resolver: false # 兼容规则见 3.4
```

**语义：**

- 仅用于解析：
    - DoH upstream URL 的 hostname（`upstreams[].endpoints[].url.host`）
    - 可选：proxy URL 的 hostname（若未来 proxy 结构化；当前为字符串 URL 时可解析其 host）
- 禁止用于普通用户查询转发/兜底。

**校验：**

- `groups` 不能为空且引用必须存在。
- 被引用组必须是 `protocol=dns`。
- 规范级建议（文档提示，不做强校验）：bootstrap 组的 DNS server 地址应为 IP:port（避免 bootstrap 再 bootstrap）。

### 3.2 upstream group 扩展：`fallback` / `failover` / `health`

```yaml
upstreams:
    - name: "secure"
      protocol: "doh"
      policy: "random"
      endpoints: [...]
      fallback: "lan" # 可选：单个后备组（本期仅支持 1 个）
      failover:
          on_rcode: [] # 默认空；可选 ["servfail", "refused"]
          max_total_time_ms: 800 # 可选：总截止时间（毫秒）
          max_groups: 2 # 可选：默认 2（primary + fallback）
          max_endpoints_per_group: 2# 可选：默认 2（同组最多换 1 次）
      health:
          failure_threshold: 2
          cooldown_seconds: 10
          success_reset: true
```

**校验要点：**

- `fallback` 引用必须存在，且禁止指向自身。
- `protocol=dns` 时仍禁止 `proxy/retry`（沿用现有校验逻辑）。
- `failover.on_rcode` 仅允许固定集合：`servfail|refused`（输入需归一化）。

### 3.3 server（DNS endpoint）扩展：`transport`

对 `protocol=dns` 的 endpoint：

```yaml
endpoints:
    - addr: 223.5.5.5:53
      transport: "udp" # 可选：udp|tcp；未配置时遵循 dns.prefer_tcp（prefer_tcp=false 时 UDP 首发，TC=1 同端点升级 TCP）
```

**语义：**

- 未配置 `transport`：由全局 `dns.prefer_tcp` 决定首发协议；若首发为 UDP 且 UDP `TC=1`，对同一上游做一次 TCP 重试以拿完整响应。
- `tcp`：只用 TCP。
- `udp`：UDP 优先；若 UDP `TC=1`，对同一上游做一次 TCP 重试以拿完整响应（`TC=1` 本身不计入 health failure；只有 TCP 也失败/超时才算 failure）。

### 3.4 `bootstrap_dns.use_system_resolver` 兼容规则（分层默认）

- 未配置 `bootstrap_dns`：等价 `use_system_resolver=true`（保持现状）。
- 配置了 `bootstrap_dns` 但未显式写 `use_system_resolver`：默认 `false`（可控/隐私优先）。

---

## 4. 运行时语义（核心算法）

### 4.1 总体流程（单次查询）

1. Router 选定 primary upstream（`target`）。
2. `UpstreamManager` 执行 `forward_with_failover(primary, query, deadline)`：
    - 在预算内按 “组 → endpoint” 进行有限次尝试
    - 只对 transport error 触发 failover；对 rcode 的 failover 需显式开启 `on_rcode`
    - 获得合法 DNS Message 立即返回（包括 `NXDOMAIN/NOERROR`）
3. 每次失败都调用 `report_failure()` 更新被动熔断状态；成功可调用 `report_success()`（若 `success_reset=true`）。
4. 输出指标与结构化日志（见第 6 节）。

### 4.2 失败分类（必须严格）

按是否“已获得合法 DNS 响应报文”分两大类：

- A. Transport error（可触发 failover，默认启用）
    - timeout / connect error / TLS error / HTTP 5xx /（未来可选 HTTP 429）
    - 说明：DNS/UDP 的 `TC=1` 不属于 transport error；应先对同一上游做 TCP 升级重试，只有 TCP 也失败/超时才算 failure。
- B. DNS 语义结果（默认不触发 failover）
    - `NOERROR`、`NXDOMAIN`：禁止 failover
    - `SERVFAIL/REFUSED`：默认不 failover；只有 `failover.on_rcode` 显式开启才允许

### 4.3 尝试顺序与去重

- 同一组内不得重复尝试同一 endpoint（请求上下文维护 `attempted_endpoints`）。
- 选择 endpoint 必须跳过 cooldown 中的 endpoint。
- 若组内所有 endpoint 均不可选（全部 cooldown 或 attempted 用尽）：
    - 若存在 fallback 且预算允许：切换到 fallback 组继续
    - 否则：返回错误

---

## 5. 实施计划（Phase 1–4，可回滚）

> 每阶段必须包含：配置校验 + 单测 + 最小集成测试（wiremock / 本地 UDP DNS stub）。

### Phase 1：配置结构与校验

交付：

- 新增 `bootstrap_dns / fallback / failover / health / transport` 结构体与 serde/validator 校验
- 引用存在校验（fallback/bootstrap）
- 错误信息可定位（指出字段路径与原因）

影响文件（预计）：

- `src/config/mod.rs`
- `src/config/upstream.rs`
- `tests/config_tests.rs`
- （可选）更新文档示例：`config.example.yaml`

回滚：

- 仅 Schema 与校验，不改运行时逻辑，安全可回滚。

### Phase 2：请求内 failover（最小版本）

交付：

- `UpstreamManager::forward()` 支持在预算内循环尝试（组→endpoint）
- 默认仅对 transport error failover；rcode 需显式开启
- endpoint 去重（attempted）

影响文件（预计）：

- `src/upstream/manager.rs`
- `tests/upstream_test.rs`（新增 failover 场景）

回滚：

- 通过配置门禁：仅当组显式配置 `fallback/failover` 才启用新逻辑，否则走旧路径。

### Phase 3：被动熔断

交付：

- `report_failure` 生效：失败阈值 + cooldown
- `select_endpoint` 跳过 cooldown
- `success_reset` 生效（成功后重置失败计数）

影响文件（预计）：

- `src/balancer.rs`（或新增健康状态模块）
- `src/upstream/manager.rs`
- `tests/upstream_test.rs`

回滚：

- `health` 默认为 None（保持现状 no-op），只有显式配置才启用熔断。

### Phase 4：Bootstrap DNS（DoH hostname 解析）

交付：

- 为 upstream DoH client 注入 reqwest 自定义 DNS resolver（实现 `reqwest::dns::Resolve`）
- TTL 缓存、超时、`use_system_resolver` 兼容

影响文件（预计）：

- `src/upstream/http_client.rs`
- `src/upstream/manager.rs`（透传 bootstrap 配置到 HTTP client 构建处）
- `src/config/mod.rs`（bootstrap_dns 读入）
- `tests/http_doh_tests.rs`（或新增集成测试）

回滚：

- `bootstrap_dns` 为可选块；缺省保持系统解析；可整体回滚该模块。

---

## 6. 可观测性（Metrics / Logging）

> 目标：可解释 + 控制 label 基数（禁止 qname/客户端 IP 等高基数维度）。

建议新增指标（在 `src/metrics.rs`）：

- `upstream_failover_total{reason, from_group, to_group}`
    - `reason`: `timeout|network_error|http_5xx|rcode_servfail|rcode_refused|no_upstream_available`
- `upstream_attempts_total{group, protocol, transport}`
- `bootstrap_dns_queries_total{result}`：`hit|miss|error`

日志（debug 级结构化字段）：

- `request_id`（若有）、`group`、`endpoint`、`attempt_idx`、`reason`、`duration_ms`、`action=retry|failover|return`

---

## 7. 测试计划（最小可验证集合）

1. 单测：failover 触发条件（transport error vs NXDOMAIN；SERVFAIL/REFUSED 需显式开启）
2. 单测：`fallback` 引用存在校验；bootstrap 引用必须是 `protocol=dns`
3. 集成：DoH 上游 A 失败（5xx/timeout）→ 自动切换到 DoH 上游 B 或 fallback DNS 组
4. 集成：被动熔断生效（连续失败进入 cooldown，cooldown 内不选）
5. 集成：bootstrap 解析 upstream hostname（禁用系统 resolver 仍可工作）
6. 集成：`transport=udp` 且 UDP 响应 `TC=1` → 在预算内触发 failover；无路径时原样返回截断响应

建议验证命令：

- `cargo fmt -- --check`
- `cargo clippy -- -D warnings`
- `cargo test`

---

## 8. 已落地实现概览（截至 2026-03-07）

> 本节用于“评审/排障/对齐现状”：列出已合入的关键行为、配置写法与指标名称，避免只停留在计划层。

### 8.1 行为与兼容性（最终）

- **向后兼容**：未配置 `fallback/failover/health/bootstrap_dns` 时，行为保持原样（单次选择、无请求内 failover、无熔断、DoH hostname 仍走系统解析）。
- **请求内 failover**：仅当该组配置了 `fallback` 或 `failover` 才启用请求内循环（组→endpoint），并在预算内尝试。
- **RCODE gating**：`SERVFAIL/REFUSED` 默认不触发 failover；需要显式配置 `failover.on_rcode`。
- **transport=udp + TC=1**：若首发为 UDP 且响应 `TC=1`，由 DNS client 对**同一端点**升级 TCP 并重试；`TC=1` 本身不计入 health failure，也不作为 failover reason。只有当 TCP 也失败/超时才会 `report_failure` 并触发后续 failover。
- **被动熔断（health）**：仅当组配置了 `health` 时生效；达到阈值进入 cooldown，cooldown 内在选择阶段跳过；`success_reset=true` 时成功会清零失败并解除 cooldown。
- **Bootstrap DNS**：仅注入 DoH upstream 的 reqwest client（以及该 client 可能使用到的 proxy hostname）；`use_system_resolver=false` 时 bootstrap 失败会直接失败，不回退系统解析。
- **代理行为变更（重要）**：上游 HTTP client 默认启用 `no_proxy()`，避免环境/系统代理影响；只有显式配置 `proxy` 时才使用代理。

### 8.2 关键代码落点（文件清单）

- 配置 Schema 与校验：`src/config/core.rs`、`src/config/upstream.rs`、`src/config/mod.rs`
- 请求内 failover：`src/upstream/manager.rs`
- DNS transport 支持：`src/upstream/dns_client.rs`
- 被动熔断：`src/balancer.rs`
- Bootstrap DNS resolver：`src/upstream/bootstrap_dns.rs`、`src/upstream/http_client.rs`、`src/main.rs`
- 指标：`src/metrics.rs`
- 主要测试：
    - 配置校验：`tests/config_tests.rs`
    - DNS transport：`tests/dns_client_integration_test.rs`
    - failover：`tests/upstream_test.rs`
    - 熔断：`tests/balancer_health_test.rs`
    - bootstrap_dns：`tests/bootstrap_dns_integration_test.rs`

### 8.3 Prometheus 指标（最终名称与 label）

- `loadants_upstream_attempts_total{upstream_protocol, upstream_transport, group, server}`
    - 语义：**同一次请求内**的“选择并尝试”次数（与 `loadants_upstream_requests_total` 的“实际网络请求次数”区分）。
- `loadants_upstream_failover_total{reason, from_group, to_group, upstream_protocol, upstream_transport, server}`
    - 语义：请求内触发“继续尝试/切换组”的次数（原因包括 `rcode_*`、`request_error` 等）。
- `loadants_bootstrap_dns_queries_total{result}`
    - `result`：`hit|miss|system|error`

> 仍然禁止把 qname / client_ip 等高基数维度写入 label。

### 8.4 示例配置（vNext）

#### 8.4.1 bootstrap_dns（仅用于 DoH/proxy hostname 解析）

```yaml
bootstrap_dns:
    groups: ["bootstrap_dns"]
    timeout: 2
    cache_ttl: 300
    prefer_ipv6: false
    use_system_resolver: false
```

#### 8.4.2 upstreams：DoH 主用 + DNS 兜底 + failover + 熔断 + transport

```yaml
upstreams:
    - name: doh_primary
      protocol: doh
      policy: roundrobin
      endpoints:
          - url: "https://dns.google/dns-query"
            method: get
            content_type: message
            weight: 1
          - url: "https://cloudflare-dns.com/dns-query"
            method: get
            content_type: message
            weight: 1
      fallback: dns_fallback
      failover:
          max_total_time_ms: 800
          max_groups: 2
          max_endpoints_per_group: 2
          on_rcode: ["servfail", "refused"]
      health:
          failure_threshold: 3
          cooldown_seconds: 30
          success_reset: true

    - name: dns_fallback
      protocol: dns
      policy: roundrobin
      endpoints:
          - addr: "223.5.5.5:53"
            weight: 1
            transport: udp
          - addr: "119.29.29.29:53"
            weight: 1
            transport: udp
      health:
          failure_threshold: 3
          cooldown_seconds: 30
          success_reset: true
```

> 注意：若 endpoint 显式 `transport: udp`，遇到 `TC=1` 时不会自动 TCP 回退，而是交给请求内 failover（或最终回退返回截断响应）。
