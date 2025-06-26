# Prometheus 监控

在生产环境中，对应用进行有效的监控是确保服务质量和快速排查问题的关键。Load Ants 通过标准的 `Prometheus` 指标为你提供了强大的可观测性能力。

### 通过 Prometheus 进行监控

Load Ants 内置了一个 Prometheus 导出器，可以暴露详细的内部状态指标，以便被 Prometheus 服务器抓取和存储。

#### 步骤一：启用 Metrics 端点

指标端点是 `admin` (健康检查与管理) 服务的一部分。要启用它，只需在你的 `config.yaml` 中配置 `admin` 块即可。

```yaml
# 健康检查与管理服务器设置（可选）
admin:
    listen: "127.0.0.1:9000" # Admin 服务监听地址和端口
```

-   **启用**: 只要 `admin` 部分被配置并启用，指标端点就会 **自动** 在 `admin` 服务的 `/metrics` 路径上可用。在这个例子中，URL 将是 `http://127.0.0.1:9000/metrics`。
-   **无需额外配置**: 你 **不能** 在 `config.yaml` 中配置路径或禁用它。它的生命周期与 `admin` 服务绑定。

> **安全警告**: 指标端点与 Admin API 使用相同的监听地址和端口。请务必遵循[安全最佳实践](./security.md#3-保护-admin-api)来保护这个端点，例如使用防火墙或反向代理，防止未授权的访问。

#### 步骤二：配置 Prometheus 来抓取指标

在你的 `prometheus.yml` 配置文件中，添加一个新的抓取任务来指向 Load Ants。

```yaml
scrape_configs:
    - job_name: "load-ants"
      static_configs:
          - targets: ["<load_ants_host>:9000"] # 将此处替换为你的 admin 服务地址
```

重启 Prometheus 后，它将开始定期从 Load Ants 拉取指标数据。

#### 关键指标深度解读

Load Ants 提供了丰富的指标，以下是几个核心指标的分组说明。所有指标均以 `loadants_` 为前缀。

##### 1. 请求处理和性能

-   **`loadants_dns_requests_total`**: 按协议（UDP/TCP）分类的已处理 DNS 请求总数。
    -   _标签_: `protocol`
-   **`loadants_dns_request_duration_seconds`**: DNS 请求处理时长的直方图。
    -   _标签_: `protocol`, `query_type`
    -   _用途_: 监控服务延迟，计算 P95/P99 响应时间。
-   **`loadants_http_requests_total`**: 按状态码分类的已处理 DoH 请求总数。
    -   _标签_: `status_code`

##### 2. 缓存效率

-   **`loadants_cache_entries`**: DNS 缓存中的当前条目数 (Gauge)。
-   **`loadants_cache_capacity`**: DNS 缓存的最大容量 (Gauge)。
-   **`loadants_cache_operations_total`**: 按操作类型分类的缓存操作总数。
    -   _标签_: `operation` (`hit`, `miss`, `insert`, `evict`, `expire`)
    -   _用途_: 计算缓存命中率 `rate(loadants_cache_operations_total{operation="hit"}[5m]) / rate(loadants_cache_operations_total{operation=~"hit|miss"}[5m])`。

##### 3. 上游解析器

-   **`loadants_upstream_requests_total`**: 发送到上游 DoH 解析器的请求总数。
    -   _标签_: `group`, `server` (服务器 URL)
-   **`loadants_upstream_errors_total`**: 上游 DoH 解析器错误总数。
    -   _标签_: `error_type`, `group`, `server`
    -   _用途_: 快速定位出问题的上游服务器或组，并设置告警。
-   **`loadants_upstream_duration_seconds`**: 上游 DoH 查询时长的直方图。
    -   _标签_: `group`, `server`
    -   _用途_: 评估不同上游解析器的性能。

##### 4. 路由策略

-   **`loadants_route_matches_total`**: 路由规则匹配总数。
    -   _标签_: `match_type` (`exact`, `wildcard`, `regex`), `target_group`, `rule_source` (`static`, `remote`), `action` (`block`, `forward`)
    -   _用途_: 精确洞察你的路由规则是如何被使用的。
-   **`loadants_route_rules_count`**: 当前活动的路由规则数量。
    -   _标签_: `match_type` (`exact`, `wildcard`, `regex`), `rule_source`

---

### 下一步

-   [➡️ 了解安全注意事项](./security.md)
-   [➡️ 查看架构设计](../architecture/index.md)
-   [➡️ 返回部署总览](./index.md)
