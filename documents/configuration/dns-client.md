# DNS 客户端配置

`dns_client` 配置块用于定义 **传统 DNS 上游（UDP/TCP，端口 53）** 的全局客户端行为。

它仅在你的某个上游组设置了 `scheme: dns` 时生效；DoH 上游与规则下载仍由 [`http_client`](./http-client.md) 控制。

---

### 示例

```yaml
dns_client:
    connect_timeout: 2 # TCP 建连超时（秒）
    request_timeout: 3 # 单次 DNS 请求超时（秒）
    prefer_tcp: false # 是否默认使用上游 TCP
    tcp_reconnect: true # TCP 请求失败后下次重连
```

---

### 参数详解

| 参数              | 类型 | 描述                                                                                                                | 默认值（未配置时） | 是否必填                         |
| :---------------- | :--- | :------------------------------------------------------------------------------------------------------------------ | :----------------- | :------------------------------- |
| `connect_timeout` | 整数 | TCP 建立连接的超时时间（秒），有效范围 `1-120`。仅对 TCP 生效。                                                     | `2`                | **是**（若 `dns_client` 块存在） |
| `request_timeout` | 整数 | 单次上游请求等待超时时间（秒），有效范围 `1-1200`。当触发 UDP→TCP 重试时，UDP 与 TCP 各自独立计时。                 | `3`                | **是**（若 `dns_client` 块存在） |
| `prefer_tcp`      | 布尔 | `true`：始终使用上游 TCP；`false`：优先 UDP，若 UDP 响应 `TC=1`（截断）则使用 TCP 重新发送同一请求并返回 TCP 响应。 | `false`            | 否                               |
| `tcp_reconnect`   | 布尔 | 当一次 TCP 请求失败时，是否丢弃该上游服务器的 TCP 连接并在下一次请求重新建立连接。                                  | `true`             | 否                               |

---

### 行为说明（与 `upstream_groups[].scheme: dns` 的关系）

要启用传统 DNS 上游，请在上游组中设置 `scheme: dns`，并在 `servers` 中使用 `addr`：

```yaml
upstream_groups:
    - name: "lan_dns"
      scheme: "dns"
      strategy: "roundrobin"
      servers:
          - addr: 192.168.1.53:53
```

> 说明：
>
> - `scheme: dns` 的上游组 **不支持** `proxy` 与 `retry`（会触发配置校验错误）。
> - UDP 的超时/IO 错误 **不会** 自动触发 TCP 重试；只有在收到 UDP 响应且 `TC=1` 时才会触发 UDP→TCP 回退（或你显式设置 `prefer_tcp=true`）。

---

### 下一步

- [➡️ 了解上游组配置（scheme=doh|dns）](./upstream-groups.md)
- [➡️ 回顾 HTTP 客户端配置（DoH 与规则下载）](./http-client.md)
- [➡️ 返回配置总览](./index.md)
