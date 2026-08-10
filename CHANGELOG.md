# Changelog

本项目所有值得注意的变更均记录于本文件。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [0.1.16] - Unreleased

### Changed

本版本对全项目日志输出进行规范化改造（依据 `specs/logging-standard.md`）：统一级别语义、消息固定短语 + kv 结构化字段、错误措辞归一、URL userinfo 脱敏。

#### 降级/变更清单（旧级别+旧消息 → 新级别+新消息）

| 位置（文件:行，参考） | 旧级别 | 旧消息（摘要） | 新级别 | 新消息 |
|---|---|---|---|---|
| server.rs:85 | warn | DNS request rate limited | **debug** | Rate limit hit |
| server.rs:127/153/202/253/285 | error | Error sending response | **debug** | Failed to send response |
| server.rs:176 | error | Failed to parse request message | **debug** | Failed to parse request message（+client_ip/protocol） |
| server.rs:258 | error | Error processing DNS request | **debug** | Failed to process request |
| server.rs:387 | info | DNS server UDP socket {i}/{N} listening | **debug** | UDP socket bound |
| handler.rs:358/387 | warn | Background refresh: cannot create cache key / no query | **debug** | Background revalidation failed（reason=no_cache_key/no_query 字段） |
| handler.rs:403/429 | warn | Background revalidation route failed / missing target | **debug** | Background revalidation failed（reason=route_match_failed/missing_target 字段） |
| handler.rs:441/452 | warn | Background revalidation cache insert/upstream failed | **debug** | Background revalidation failed（reason=cache_insert_failed/upstream_failed 字段） |
| handler.rs:443 | **info** | Background revalidation completed | **debug** | Background revalidation completed（硬约束 #4） |
| handler.rs:484 | warn | Route matching failed | **debug** | Failed to match route |
| handler.rs:516 | error | Route rule configuration error: Forward action missing target | **debug** | Failed to resolve forward target（reason=target_missing 字段） |
| handler.rs:542 | error | Upstream request failed | **error（保留级别，重构位置）** | Failed to forward request to upstream（仅最终 SERVFAIL 时；stale 救回 → debug） |
| handler.rs:556 | warn | Upstream failure, serving stale cached response | **debug** | Stale response served after upstream failure |
| handler.rs:584 | warn | Cache insertion failed | **debug** | Failed to insert cache entry |
| cache.rs:756/772/788/803 | warn | Failed to parse ... during restore × 4 | **debug** | Failed to restore cache entry（reason=parse_name/parse_record_type/decode_hex/parse_message 字段） |
| cache.rs:883 | warn | CNAME chain depth limit reached | **debug** | CNAME chain depth limit reached（kv） |
| cache.rs:903/924/957/974 | warn | Rejected N record(s) with mismatched domain × 4 | **debug** | Rejected mismatched records section=.. |
| upstream/manager.rs:190/206/242/271/342 | error | Upstream group not found / Failed to select / Invalid server type / HTTP client not found | **debug** | Failed to select upstream path（reason=group_not_found/select_failed/invalid_server_type/http_client_missing 字段） |
| upstream/dns_client.rs:169 | warn | Failed to create randomized DNS name | **debug** | Failed to randomize query name |
| upstream/dns_client.rs:286 | warn | 0x20 strict mode: discarding UDP response... | **debug** | 0x20 strict fallback to TCP |
| upstream/dns_client.rs:459 | warn | TCP connection pool full, evicted oldest | **debug** | TCP connection pool evicted oldest connection |
| upstream/json.rs × 11 | warn | Failed to parse/Invalid/Unsupported record... | **debug** | Failed to parse ...（kv） |
| doh/handlers.rs:113 | warn | Upstream processing failed, returning DNS SERVFAIL | **debug** | Failed to process DNS query, returning SERVFAIL per RFC 8484 |
| doh/handlers.rs:628 | error | Failed to process DoH request | **debug** | DoH request failed |
| doh/handlers.rs:636 | warn | Processed DoH request with an unspecified error | **debug** | DoH request failed（reason=unspecified 字段） |
| remote_rule/mod.rs:157/199 | **error** | Failed to load remote rule source / loader | **warn** | 消息不变 |
| main.rs:328 | info | Starting scheduled remote rules reload | **debug** | Remote rules reload started |
| doh/server.rs:134 | info | DoH server received shutdown signal | **debug** | （内部信号，消息不变） |

#### 升级/新增清单

| 位置 | 原状态 | 新级别 | 新消息 |
|---|---|---|---|
| admin.rs auth_middleware 401 | 静默（P0-1） | **warn** | Admin authentication failed client_ip=.. path=.. method=.. |
| doh/handlers.rs 限流 × 3 | 静默 | **debug** | Rate limit hit protocol=doh method=.. |
| doh/handlers.rs Query 提取器拒绝 | 静默 | **debug** | Failed to parse query parameters |
| upstream/manager.rs 两处 Ok 分支 | 静默 | **debug** | Upstream response received（rcode + answers） |
| upstream/manager.rs 组初始化 | 静默 | **info** | Upstream group ready group=.. scheme=.. strategy=.. servers=N |
| bootstrap.rs 空路由表 | 静默 | **error** | Routing table is empty, all queries will be refused |
| main.rs 配置摘要 | 单行 `{:?}` | **info × 2** | Configuration loaded / Configuration summary |

#### 保留清单（级别冻结）

| 位置 | 级别 | 依据 |
|---|---|---|
| balancer.rs:131/140 熔断迁移 | warn/info | G1/AC-1.1 签署契约（§3.4） |
| cache.rs:472 投毒检测 | warn | 安全例外 SEC-1 |
| dns_client.rs:188/201 0x20 校验失败 | warn | 安全例外 SEC-2 |
| manager.rs:303/404 deny_answers 全过滤 | warn | 安全例外 SEC-3 |
| doh/server.rs:112 无 TLS | warn | WARN=状态级降级（已评审） |
| bootstrap.rs:32 / remote_rule/mod.rs:94/117/213 / main.rs:368/429/436 | warn | WARN=加载失败有 fallback/自愈 |
| main.rs 各启动失败 error! + subsystem 绑定失败 error! | error | ERROR=启动失败 |

#### WARN/ERROR 语义变更说明

- 新语义：**ERROR** = 非请求维度的服务能力丧失（启动失败、全部上游组不可用、路由表为空等，需运维立即介入）；**WARN** = 状态转换/降级自愈（熔断 trip、远程规则源加载失败但有 fallback、关停超时等）；**请求实例失败一律 DEBUG + metrics**，不再出现在 WARN/ERROR。
- 请求路径唯一的 error! 聚合点位于 handler.rs `handle_forward`：仅在上游重试耗尽且 stale fallback 未救回、最终返回 SERVFAIL 时发出。
- **安全例外（保留 WARN）**：缓存投毒检测（cache.rs）、0x20 大小写校验失败（dns_client.rs）、deny_answers 过滤后 A/AAAA 全部被拒（manager.rs）三类事件属「外部异常状态的发现」而非请求实例失败，且无 metric 覆盖，是 DNS 劫持/污染的唯一证据链，保留 WARN 且禁止降级。
- dns_client.rs:286「0x20 strict 模式丢弃 UDP 回退 TCP」被排除出安全例外：该日志描述的是**传输行为**（校验失败后的后续处理动作），本身不证明劫持，降级为 DEBUG。
- src/balancer.rs 熔断器状态迁移日志（`Circuit breaker tripped`/`Circuit breaker transition`）为已签署契约（G1/AC-1.1），本次**零改动**（级别、消息、字段、发出条件、metric 联动全部冻结）。

#### 告警迁移提示

原先依赖日志告警的请求级事件（限流、发送失败、上游单请求失败、DoH 请求失败等）已迁移至 metrics，日志级别降为 DEBUG。建议将告警接入以下既有指标：

- `loadants_upstream_errors_total`
- `loadants_upstream_health_state`
- `loadants_circuit_breaker_transitions_total`
- `loadants_dns_request_errors_total`
- `loadants_stale_fallback_total`
- `loadants_http_request_errors_total`
- `loadants_tcp_pool_connections`

安全例外三类 WARN（缓存投毒检测 / 0x20 校验失败 / deny_answers 全过滤）无 metric 覆盖，仍可直接基于日志告警。

#### 格式维持确认

- UTC 微秒时间戳维持不变（默认 timer，未引入本地时区）。
- `with_ansi(false)` 无颜色输出维持不变。
- Full 纯文本格式维持不变（未引入 JSON 输出），供依赖日志解析的外部工具确认。

#### 已知限制

- 缓存投毒检测（SEC-1）暂不携带 `group` 上下文：`insert()` 调用链未穿透 group，改签名链属过度设计，本轮标注为已知限制。
- `handle_json_get`（DoH JSON GET）无 per-request span，保持现状。
