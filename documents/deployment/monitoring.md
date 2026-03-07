# Prometheus 监控

在生产环境中，对应用进行有效的监控是确保服务质量和快速排查问题的关键。Load Ants 通过标准的 `Prometheus` 指标为你提供了强大的可观测性能力。

### 通过 Prometheus 进行监控

Load Ants 内置了一个 Prometheus 导出器，可以暴露详细的内部状态指标，以便被 Prometheus 服务器抓取和存储。

#### 步骤一：启用 Metrics 端点

指标端点是 `admin`（健康检查与管理）服务的一部分。当前版本中，`admin` 服务默认会启动并监听在 `127.0.0.1:9000`；你可以通过配置 `admin` 块来修改其监听地址。

```yaml
# 健康检查与管理服务器设置（可选）
admin:
    listen: "127.0.0.1:9000" # Admin 服务监听地址和端口
```

- **访问路径**: 指标端点会 **自动** 在 `admin` 服务的 `/metrics` 路径上可用。在这个例子中，URL 将是 `http://127.0.0.1:9000/metrics`。
- **无需额外配置**: 你 **不能** 在 `config.yaml` 中配置路径或禁用它。它的生命周期与 `admin` 服务绑定。

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

- **`loadants_dns_requests_total`**: 按协议（UDP/TCP）分类的已处理 DNS 请求总数。
    - _标签_: `protocol`
- **`loadants_dns_request_duration_seconds`**: DNS 请求处理时长的直方图。
    - _标签_: `protocol`, `query_type`
    - _用途_: 监控服务延迟，计算 P95/P99 响应时间。
- **`loadants_dns_handler_duration_seconds`**: DNS 请求处理器耗时的直方图（不区分 UDP/TCP/DoH 入口）。
    - _标签_: `stage` (`cached`, `resolved`), `query_type`
    - _用途_: 判断缓存命中路径与解析路径的耗时差异；结合入口侧延迟指标定位瓶颈在“入口网络/协议”还是“处理器内部”。
- **`loadants_http_requests_total`**: 按状态码分类的已处理 DoH 请求总数。
    - _标签_: `status_code`

##### 2. 缓存效率

- **`loadants_cache_entries`**: DNS 缓存中的当前条目数 (Gauge)。
- **`loadants_cache_capacity`**: DNS 缓存的最大容量 (Gauge)。
- **`loadants_cache_operations_total`**: 按操作类型分类的缓存操作总数。
    - _标签_: `operation` (`hit`, `miss`, `insert`, `insert_error`, `clear`)
    - _用途_: 计算缓存命中率 `rate(loadants_cache_operations_total{operation="hit"}[5m]) / rate(loadants_cache_operations_total{operation=~"hit|miss"}[5m])`。
- **`loadants_cache_ttl_seconds`**: 缓存条目 TTL 的直方图（秒）。
    - _标签_: `source` (`original`, `min_ttl`, `adjusted`, `negative_ttl`)
    - _用途_: 观察 TTL 分布，以及 `min_ttl` / 负向缓存是否频繁介入。

##### 3. 上游解析器

> **指标升级说明（Breaking Change）**  
> 新版本已将 `loadants_upstream_*` 系列指标从“仅 DoH”升级为“DoH + DNS(UDP/TCP)”通用指标，并新增了两个标签维度：
>
> - `upstream_protocol`: `doh|dns`
> - `upstream_transport`: `http|udp|tcp`  
>   因此如果你之前在 Grafana/PromQL 中只按 `group/server` 聚合或过滤，需要把新标签纳入查询（或用 `sum by (...)` 忽略它们）。

- **`loadants_upstream_requests_total`**: 发送到上游解析器的请求总数。
    - _标签_: `upstream_protocol`, `upstream_transport`, `group`, `server`
    - _说明_:
        - `doh/http`：DoH 上游（`server` 通常为 Host）。
        - `dns/udp|dns/tcp`：传统 DNS 上游（`server` 通常为 IP 字符串）。
        - 当 `dns.prefer_tcp=false` 且 UDP 响应 `TC=1` 触发回退时：**同一条逻辑请求可能会分别产生一条 `dns/udp` 与一条 `dns/tcp` 的请求计数**（按“尝试次数”计数，这是预期行为）。
- **`loadants_upstream_errors_total`**: 上游解析器错误总数。
    - _标签_: `upstream_protocol`, `upstream_transport`, `error_type`, `group`, `server`
    - _用途_: 快速定位出问题的上游服务器或组，并设置告警。
- **`loadants_upstream_duration_seconds`**: 上游查询时长的直方图。
    - _标签_: `upstream_protocol`, `upstream_transport`, `group`, `server`
    - _用途_: 评估不同上游解析器的性能；对 `dns` 上游可以分别观察 `udp` 与 `tcp` 的延迟分布。

**PromQL 迁移示例**

- 旧：只看某个组的上游请求速率（旧版无新标签）
    - `rate(loadants_upstream_requests_total{group="public"}[5m])`
- 新：保留新标签，分别看不同上游协议/传输
    - `rate(loadants_upstream_requests_total{group="public"}[5m])`
- 新：忽略 `upstream_protocol/upstream_transport`，得到与旧版更接近的聚合口径
    - `sum by (group, server) (rate(loadants_upstream_requests_total[5m]))`

##### 4. 路由策略

- **`loadants_route_matches_total`**: 路由规则匹配总数。
    - _标签_: `rule_type` (`exact`, `wildcard`, `regex`), `target_group`, `rule_source`, `action` (`block`, `forward`)
    - _用途_: 精确洞察你的路由规则是如何被使用的。
    - _备注_: 远程规则在加载后会被合并进路由引擎；当前版本的匹配计数中，`rule_source` 可能统一为 `static`（即使规则源来自 `rules.remote`）。
- **`loadants_route_rules_count`**: 当前活动的路由规则数量。
    - _标签_: `rule_type` (`exact`, `wildcard`, `regex`), `rule_source`

---

### 下一步

- [➡️ 了解安全注意事项](./security.md)
- [➡️ 查看架构设计](../architecture/index.md)
- [➡️ 返回部署总览](./index.md)
