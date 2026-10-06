# Prometheus 监控

Load Ants 通过 `/metrics` 暴露 Prometheus 指标，用于观察请求量、缓存命中、上游健康和路由行为。

## 启用方式

`/metrics` 端点由 Admin 服务器提供。默认监听地址是 `127.0.0.1:9000`，可通过配置修改：

```yaml
admin:
  listen: "127.0.0.1:9000"
```

对应抓取地址示例：

```yaml
scrape_configs:
  - job_name: "load-ants"
    static_configs:
      - targets: ["<load_ants_host>:9000"]
```

若配置了 `admin.auth.token`，抓取时需携带 Bearer token：

```yaml
scrape_configs:
  - job_name: "load-ants"
    bearer_token: "your_secret_token"
    static_configs:
      - targets: ["<load_ants_host>:9000"]
```

建议只在受控网络内暴露该端点，并结合防火墙或反向代理限制访问。

## 核心指标

### 请求处理

- `loadants_dns_requests_total`
  - 标签：`protocol`
- `loadants_dns_request_duration_seconds`
  - 标签：`protocol`, `query_type`
- `loadants_dns_request_processing_duration_seconds`
  - 标签：`processing_stage`, `query_type`
  - `processing_stage` 取值为 `cached`（缓存命中）或 `resolved`（经上游解析）
- `loadants_dns_request_errors_total`
  - 标签：`error_type`
- `loadants_http_requests_total`
  - 标签：`status_code`
- `loadants_http_request_duration_seconds`
  - 标签：`query_type`, `status_code`
- `loadants_http_request_errors_total`
  - 标签：`error_type`
- `loadants_dns_query_type_total`
  - 标签：`type`（记录类型，如 `A`、`AAAA`、`MX`、`TXT`，未识别类型归为 `OTHER`）
- `loadants_dns_response_codes_total`
  - 标签：`rcode`（响应码，如 `NOERROR`、`NXDOMAIN`、`SERVFAIL`，未识别响应码归为 `OTHER`）

### 运行状态

- `loadants_inflight_requests`
  - 无标签；当前正在处理中的 DNS 请求数
- `loadants_tcp_pool_connections`
  - 无标签；当前连接池中的 TCP 连接数

### 缓存

- `loadants_cache_entries`
- `loadants_cache_capacity`
- `loadants_cache_operations_total`
  - 标签：`operation`
- `loadants_cache_ttl_seconds`
  - 标签：`source`

### 上游解析

- `loadants_upstream_requests_total`
  - 标签：`upstream_protocol`, `upstream_transport`, `group`, `server`
- `loadants_upstream_errors_total`
  - 标签：`upstream_protocol`, `upstream_transport`, `error_type`, `group`, `server`
- `loadants_upstream_duration_seconds`
  - 标签：`upstream_protocol`, `upstream_transport`, `group`, `server`
- `loadants_upstream_health_state`
  - 标签：`group`, `server`
  - 值语义：`0`=Unhealthy，`1`=HalfOpen，`2`=Healthy
  - **破坏性变更提示**（2026-08）：旧版本中 `0` 表示 Healthy，升级后监控面板与告警规则需同步更新阈值
- `loadants_circuit_breaker_transitions_total`
  - 标签：`group`, `state_from`, `state_to`
  - 统计上游健康状态迁移次数（如 Healthy→Unhealthy、Unhealthy→HalfOpen、HalfOpen→Healthy/Unhealthy）
- `loadants_stale_fallback_total`
  - 标签：`group`
  - 统计上游转发失败时以过期缓存兜底响应的次数

说明：

- `upstream_protocol` 取值为 `doh` 或 `dns`
- `upstream_transport` 取值为 `http`、`udp` 或 `tcp`
- 当传统 DNS 上游先走 UDP、再因 `TC=1` 回退到 TCP 时，同一逻辑请求会产生两条上游请求计数（UDP 与 TCP 各一条），这是预期行为

### 健康检查端点

`/health/live` 与 `/health/ready` 由 Admin 服务器提供，可用于探针与告警：

- `/health/live`：存活探针（livenessProbe）端点，进程存活即返回 200
- `/health/ready`：就绪探针（readinessProbe）端点，任一上游组存在服务器（servers>0）且全部不健康（healthy=0）时返回 503；其余情况返回 200

### 路由策略

- `loadants_route_matches_total`
  - 标签：`rule_type`, `target_group`, `rule_source`, `action`
  - `rule_source` 会按真实来源写入 `static` 或 `remote`
  - 若规则来自远程源，内部还会保留来源标识，便于后续实现规则可解释性或热更新扩展

- `loadants_route_rules_count`
  - 标签：`rule_type`, `rule_source`
  - 统计当前编译后有效规则数，不再把远程规则统一记成 `static`

## PromQL 示例

查看某个上游组的请求速率：

```promql
rate(loadants_upstream_requests_total{group="public"}[5m])
```

忽略 `upstream_protocol` 和 `upstream_transport` 做聚合：

```promql
sum by (group, server) (rate(loadants_upstream_requests_total[5m]))
```

查看路由匹配中远程规则的命中量：

```promql
rate(loadants_route_matches_total{rule_source="remote"}[5m])
```

## 告警规则

仓库提供了一份即用型 Prometheus 告警规则示例：[`examples/prometheus/alert-rules.yml`](https://github.com/shengyanli1982/load-ants/blob/main/examples/prometheus/alert-rules.yml)，覆盖上游健康、熔断、请求错误与连接池共 7 个告警相关指标。

在 `prometheus.yml` 中挂载后重载配置即可生效：

```yaml
rule_files:
  - "/etc/prometheus/rules/loadants-alerts.yml"  # 替换为规则文件的实际路径
```

规则概览：

| 告警 | 指标 | 触发条件（建议值） | severity |
|------|------|--------------------|----------|
| `LoadAntsUpstreamServerUnhealthy` | `loadants_upstream_health_state` | `== 0`（Unhealthy）持续 5m | warning |
| `LoadAntsUpstreamGroupAllDown` | `loadants_upstream_health_state` | 组内全部服务器 `== 0` 持续 2m | critical |
| `LoadAntsCircuitBreakerTripped` | `loadants_circuit_breaker_transitions_total` | 5m 内出现迁移到 `unhealthy` | warning |
| `LoadAntsUpstreamErrorRateHigh` | `loadants_upstream_errors_total` | 按 `group`/`server` 聚合 rate > 1/s 持续 5m | warning |
| `LoadAntsStaleFallbackSustained` | `loadants_stale_fallback_total` | 按 `group` 聚合 10m 内 increase > 10 | warning |
| `LoadAntsDnsRequestErrorsHigh` | `loadants_dns_request_errors_total` | 按 `error_type` 聚合 rate > 0.5/s 持续 5m | warning |
| `LoadAntsDnsRoutingConfigError` | `loadants_dns_request_errors_total` | `error_type=~"route_error\|missing_target"` 10m 内 increase > 0 | warning |
| `LoadAntsHttpRequestErrorsHigh` | `loadants_http_request_errors_total` | 按 `error_type` 聚合 rate > 0.5/s 持续 5m | warning |
| `LoadAntsTcpPoolNearCapacity` | `loadants_tcp_pool_connections` | `> 204`（默认上限 256 的 80%）持续 5m | warning |

注意事项：

- 表中所有阈值均为**保守建议值**，请按实际流量规模调整（低流量环境可将速率类阈值下调，或改用错误比例判断）。
- `LoadAntsUpstreamServerUnhealthy` 依赖 2026-08 后的值语义（`0`=Unhealthy，见上文[上游解析](#上游解析)），从旧版本升级时请先核对规则方向。
- `LoadAntsTcpPoolNearCapacity` 的阈值对应默认 `max_tcp_connections=256`；若自定义了该配置，请同步按上限的 80% 调整。
- 缓存投毒检测、0x20 大小写校验失败、`deny_answers` 全过滤三类**安全事件无 metric 覆盖**，仍以 WARN 级别写入日志（见 `specs/logging-standard.md` 安全例外条目），请通过日志告警通道（如 Loki/Alertmanager 日志路由）接入，不要依赖本规则文件。

## 下一步

- [➡️ 了解安全注意事项](./security.md)
- [➡️ 查看架构设计](../architecture/index.md)
- [➡️ 返回部署总览](./index.md)
