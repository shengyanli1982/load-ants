# 上游服务器管理

上游（Upstream）是你的 DNS 查询经过 Load Ants 路由决策（对 DoH 上游还包括加密）后的最终目的地。上游服务器按上游组组织和管理；合理配置上游组，是保证 DNS 解析服务高可用和高性能的关键。

### 一个类比：分配任务的“工作小组”

你可以将 Load Ants 的上游管理机制想象成一个办公室里的项目管理场景：

- **上游服务器（`servers`）**：就像是办公室里的一个个“组员”。组员可以是 DNS-over-HTTPS（DoH）服务器（以 URL 指定），也可以是传统 DNS 服务器（以 `IP:端口` 指定）。
- **上游组（`upstream_groups`）**：就像是为了完成特定目标而建立的“工作小组”。你可以创建多个小组，例如“谷歌组”、“阿里组”、“抗污染专用组”等。
- **负载均衡策略（`strategy`）**：就像是“小组组长”分配任务的方式。组长需要决定下一个进来的任务（DNS 查询）交给哪个组员处理。

Load Ants 就是这位高效的“管理者”，根据你设定的策略，将查询分发给组内的各个服务器。

### 负载均衡策略（`strategy`）

为每个上游组选择合适的策略，可以满足性能、隐私或故障转移等不同目标。

| 策略名称         | 工作方式                                                                                                                  | 最佳适用场景                                                                                                  |
| :--------------- | :------------------------------------------------------------------------------------------------------------------------ | :------------------------------------------------------------------------------------------------------------ |
| **`roundrobin`** | **轮询**：像发牌一样，按顺序将请求依次分配给组里的每个服务器。1 号发完发 2 号，2 号发完发 3 号，一轮结束后再从 1 号开始。 | 当组内所有服务器性能相近时，用它来平均分配负载是最公平、最简单的。                                            |
| **`weighted`**   | **加权**：根据你为每个服务器设定的“权重”（`weight`）来分配请求。权重越高的服务器，接到的请求就越多。                    | 当组内服务器性能或可靠性不同时。例如，你可以让一台高性能的主服务器承担 80% 的流量，另一台备用服务器承担 20%。 |
| **`random`**     | **随机**：完全随机地从组里挑选一个服务器来处理请求，没有任何固定顺序。                                                    | 当你希望增强隐私性时。随机选择可以打乱查询模式，让外部更难追踪你的 DNS 行为。                                 |

无论选择哪种策略，Load Ants 都会通过熔断器对组内每台服务器做健康检测：一台服务器连续失败达到阈值（默认 3 次）后会被熔断并标记为不可用，冷却期（默认 30 秒）内不会被选中；冷却期过后进入半开状态，Load Ants 允许用真实请求探测，探测成功即恢复健康。当某台服务器的请求失败时，它会在组内自动故障转移，改用其他可用服务器重试，直到成功或组内服务器全部尝试完毕。

### 连接到私有服务：认证支持

除了连接公共 DoH 服务器，Load Ants 还支持与需要认证的私有 DoH 提供商安全通信，适合企业内部或个人搭建的私有 DNS 服务。

- **支持的认证方式**：
    - **HTTP 基本认证（`Basic Auth`）**：用用户名和密码验证。
    - **Bearer 令牌认证（`Bearer Token`）**：用一个访问令牌验证。

> 说明：认证仅适用于 `scheme: doh` 的上游服务器条目（传统 DNS 上游不涉及 HTTP 认证）。

你可以在[上游组配置](../configuration/upstream-groups.md)中为特定的 DoH 服务器条目添加认证信息。

除认证外，上游组级别还支持几项可选能力：

- `proxy`：为整个组指定出站代理（仅 DoH 组）。
- `retry`：控制请求失败后的重试策略（仅 DoH 组）。
- `tls_verify`：控制是否校验上游的 TLS 证书（仅 DoH 组生效）。
- `deny_answers`：过滤应答中落在指定 CIDR 内的 A/AAAA 记录；若记录被全部过滤，该次查询返回 `SERVFAIL`。

另需注意：DoH 上游的 `content_type` 为 `json` 时只支持 GET 方法；将 `json` 与 POST 搭配使用，会在启动期的配置校验中直接报错。

### 资源优化：按组独立的连接池

为降低延迟并减少资源消耗，Load Ants 按上游类型复用连接资源：

- 对于 `scheme: doh`：**每个上游组**独立持有一个 HTTP 客户端及其连接池，高效复用与该组内上游服务器的 TCP/TLS 连接；不同上游组之间的连接池互不共享。
- 对于 `scheme: dns`：内部会复用与上游服务器的 TCP 连接，并支持在 TCP 请求失败后按需重连（见 `dns_client.tcp_reconnect`）。`scheme: dns` 组还支持 0x20 大小写随机化（仅对该 scheme 生效）：通过随机化查询域名的大小写并校验响应来防范 DNS 伪造；在 `strict` 模式下校验失败会丢弃该 UDP 响应并自动回退 TCP 重试。

这意味着，即使你配置了数十个不同的上游服务器，Load Ants 也会复用底层连接资源（DoH 为 TCP/TLS 连接，传统 DNS 为 TCP 连接）。这带来了几个好处：

- **减少延迟**：避免了每个新请求都重新做 TCP 和 TLS 握手的开销。
- **降低 CPU 和内存使用**：更少的连接数意味着更少的系统资源占用。
- **提升吞吐量**：连接复用是实现高并发 DoH 请求的关键。

这个优化过程自动且透明，无需额外配置。

---

### 配置示例

下面是一个包含多个上游组和不同策略的配置示例：

```yaml
upstream_groups:
    # 上游组 1: 公共 DoH，用于日常使用
    - name: "public_doh"
      scheme: "doh"
      # 策略: 轮询，平均分配请求
      strategy: "roundrobin"
      servers:
          - url: "https://223.5.5.5/dns-query" # 阿里 DNS
          - url: "https://1.12.12.12/dns-query" # 腾讯 DNS

    # 上游组 2: 高性能服务器组，主备模式
    - name: "high_performance"
      scheme: "doh"
      # 策略: 加权，主服务器承担 80% 的请求
      strategy: "weighted"
      servers:
          - url: "https://main-server.com/dns-query"
            weight: 80 # 主服务器，权重 80
          - url: "https://backup-server.com/dns-query"
            weight: 20 # 备用服务器，权重 20

    # 上游组 3: 注重隐私的服务器组
    - name: "private_dns"
      scheme: "doh"
      # 策略: 随机选择，打乱查询模式
      strategy: "random"
      servers:
          - url: "https://dns.google/dns-query"
          - url: "https://cloudflare-dns.com/dns-query"

    # 上游组 4: 传统 DNS（UDP/TCP）上游，用于局域网/自建 DNS
    - name: "lan_dns"
      scheme: "dns"
      strategy: "roundrobin"
      servers:
          - addr: 192.168.1.53:53

# 在路由规则中，你可以根据需要将不同的域名指向这些不同的上游组
static_rules:
    - match: "wildcard"
      patterns: ["*.internal.corp"]
      action: "forward"
      target: "high_performance" # 内部服务使用高性能组

    - match: "wildcard"
      patterns: ["*"]
      action: "forward"
      target: "public_doh" # 其他所有查询使用公共 DoH 组
```

把服务器分组并应用不同策略，你可以构建一套可靠、高效的定制化 DNS 解析系统。

---

### 下一步

- [➡️ 学习如何配置上游组](../configuration/upstream-groups.md)
- [➡️ 了解什么是 DNS-over-HTTPS（DoH）](./doh.md)
- [➡️ 返回核心概念概览](./index.md)
