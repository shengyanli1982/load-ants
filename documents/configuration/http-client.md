# HTTP 客户端配置

`http_client` 配置块定义全局的 HTTP 客户端行为。此处的设置影响所有出站 HTTP 请求，包括：

- 上游组（`upstream_groups`）发出的 DoH（DNS over HTTPS）请求
- 远程规则（`remote_rules`）下载规则文件时的 HTTP 请求

> 如果你使用了传统 DNS 上游（`upstream_groups[].scheme: dns`），其 UDP/TCP 行为由 [`dns_client`](./dns-client.md) 单独控制，不受 `http_client` 影响。

按需调整这些参数，可以优化连接的性能和可靠性。

### 示例

```yaml
http_client:
    connect_timeout: 3
    request_timeout: 5
    idle_timeout: 10
    keepalive: 30
    agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
```

### 参数详解

| 参数              | 类型   | 描述                                                                                                  | 默认值 | 是否必填                          |
| :---------------- | :----- | :---------------------------------------------------------------------------------------------------- | :----- | :-------------------------------- |
| `connect_timeout` | 整数   | 建立新连接（TCP 握手）的超时时间（秒），有效范围 `1-120`。                                            | `3`                | **是**（若 `http_client` 块存在） |
| `request_timeout` | 整数   | 单次请求的总超时时间（秒，从建立连接到接收完整响应），有效范围 `1-1200`。                             | `5`                | **是**（若 `http_client` 块存在） |
| `idle_timeout`    | 整数   | 连接池中空闲连接的最大存活时间（秒），有效范围 `5-1800`。超过此时间未被使用的连接将被关闭以释放资源。 | `10`               | 否                                |
| `keepalive`       | 整数   | TCP Keepalive 探测间隔（秒），有效范围 `5-600`。有助于维持长连接并及时发现失效连接。                  | `30`               | 否                                |
| `agent`           | 字符串 | `User-Agent` 请求头。你可以将其设置为任意值，或不设置（不设置时由 HTTP 客户端保持默认行为）。         | （不设置）         | 否                                |

> ✨ **专家提示**：
>
> - 在网络状况不佳的环境中，可以调大 `connect_timeout` 和 `request_timeout`，避免网络波动导致请求超时失败。
> - `idle_timeout` 和 `keepalive` 是优化连接池性能的关键参数，大多数场景使用默认值即可。如果你需要与大量不同的上游服务器通信，且每个服务器的请求频率低，可以调低 `idle_timeout` 加快回收空闲连接。

---

### 下一步

- [➡️ 了解上游组配置](./upstream-groups.md)
- [➡️ 回顾 DoH 概念](../concepts/doh.md)
- [➡️ 返回配置总览](./index.md)
