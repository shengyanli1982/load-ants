# Prometheus 监控

Load Ants 通过 `/metrics` 暴露 Prometheus 指标，用于观察请求量、缓存命中、上游健康和路由行为。

## 启用方式

`/metrics` 端点挂在 `admin` 服务上。默认监听地址是 `127.0.0.1:9000`，可通过配置修改：

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

建议只在受控网络内暴露该端点，并结合防火墙或反向代理限制访问。

## 核心指标

### 请求处理

- `loadants_dns_requests_total`
  - 标签：`protocol`
- `loadants_dns_request_duration_seconds`
  - 标签：`protocol`, `query_type`
- `loadants_http_requests_total`
  - 标签：`status_code`

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

说明：

- `upstream_protocol` 取值为 `doh` 或 `dns`
- `upstream_transport` 取值为 `http`、`udp` 或 `tcp`
- 当传统 DNS 上游先走 UDP、再因 `TC=1` 回退到 TCP 时，同一逻辑请求可能产生两条上游请求计数，这是预期行为

### 路由策略

- `loadants_route_matches_total`
  - 标签：`rule_type`, `target_group`, `rule_source`, `action`
  - `rule_source` 会按真实来源写入 `static` 或 `remote`
  - 若规则来自远程源，内部还会保留来源标识，便于后续做 explainability 或热更新扩展

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

## 下一步

- [➡️ 了解安全注意事项](./security.md)
- [➡️ 查看架构设计](../architecture/index.md)
- [➡️ 返回部署总览](./index.md)
