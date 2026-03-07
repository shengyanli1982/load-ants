# 上游组配置

`upstreams` 是 Load Ants 配置中至关重要的一环。它定义了你的 DNS 查询最终将被发送到哪些上游解析器（**DoH** 或 **传统 DNS（UDP/TCP）**），以及如何在高可用、高性能和高隐私之间取得平衡。

本配置项是一个**列表**，这意味着你可以定义多个独立配置的"上游组"。

想要深入了解上游管理和负载均衡策略背后的理念吗？请阅读 [**核心概念：上游服务器管理**](../concepts/upstream.md)。

### 顶级参数 (上游组级别)

#### 示例

```yaml
upstreams:
    - name: "google_public"
      protocol: "doh"
      policy: "random"
      endpoints:
          # ... endpoint 列表在这里定义 ...
      retry:
          attempts: 3
          delay: 2
      proxy: "http://127.0.0.1:7890"
```

#### 参数详解

| 参数        | 类型   | 描述                                                                                                                                                                                                            | 默认值 | 是否必填 |
| :---------- | :----- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :----- | :------- |
| `name`      | 字符串 | 为这个上游组指定一个唯一的名称。这个名称将在路由规则（`rules.static` 或 `rules.remote`）的 `upstream` 字段中被引用。                                                                                            | -      | **是**   |
| `protocol`  | 字符串 | 上游组类型。可选值：`doh`（DoH 上游）或 `dns`（传统 DNS 上游，UDP/TCP）。                                                                                                                                       | `doh`  | 否       |
| `policy`    | 字符串 | 定义该组内端点的负载均衡策略。可选值为：`roundrobin` (轮询), `weighted` (加权轮询), `random` (随机)。                                                                                                           | -      | **是**   |
| `max_concurrent` | 整数 | (可选) 组级并发保护上限。达到上限时，本组会立即拒绝新的上游尝试并返回错误（最终客户端表现为 `SERVFAIL`）。计数口径为 **一次端点尝试**（实现位于 `UpstreamManager::forward_selected()`）。 | - | 否 |
| `endpoints` | 列表   | 定义本组包含的一个或多个上游端点。其条目结构取决于 `protocol`（DoH 使用 `url`，DNS 使用 `addr`）。详见下方的 `endpoints` 参数详解。                                                                             | -      | **是**   |
| `retry`     | 对象   | (可选，仅 `protocol: doh`) 本组上游请求的重试策略。详见下方 `retry` 参数详解。                                                                                                                                  | -      | 否       |
| `proxy`     | 字符串 | (可选，仅 `protocol: doh`) 为**整个组**的端点指定一个出站代理。所有发往该组内端点的 DoH 请求都将通过此代理。支持 `http`, `https` 和 `socks5` 协议。例如: `http://user:pass@host:port` 或 `socks5://host:port`。 | -      | 否       |

---

### `endpoints` 参数详解

`endpoints` 是一个列表，列表中的每个对象都代表一个上游端点。其条目结构由 `protocol` 决定：

- `protocol: doh`：条目使用 `url`（DoH 服务器 URL）。
- `protocol: dns`：条目使用 `addr`（传统 DNS 的 `IP:端口`，通常为 `:53`）。

> 说明：
>
> - 如果你不填写 `protocol`，默认按 `doh` 处理。
> - 若 `protocol: dns`，该组不支持 `proxy` 与 `retry`（会触发配置校验错误）。

#### `protocol: doh`（DoH 端点条目）

| 参数           | 类型   | 描述                                                                                                                                                                   | 默认值      | 是否必填 |
| :------------- | :----- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :---------- | :------- |
| `url`          | 字符串 | DoH 服务器的完整 URL。                                                                                                                                                 | -           | **是**   |
| `weight`       | 整数   | (可选) 端点的权重，仅在组的 `policy` 为 `weighted` 时生效。权重越高的端点，被选中的概率就越大。                                                                        | `1`         | 否       |
| `method`       | 字符串 | (可选) 与该服务器通信时使用的 HTTP 方法。可选值为 `get` 或 `post`。                                                                                                    | `"post"`    | 否       |
| `content_type` | 字符串 | (可选) DoH 请求的内容类型。可选值为 `message` (对应 `application/dns-message`) 或 `json` (对应 `application/dns-json`)。注意：如果设为 `json`，`method` 必须为 `get`。 | `"message"` | 否       |
| `auth`         | 对象   | (可选) 访问此特定服务器所需的认证配置。详见下方的 `auth` 参数详解。                                                                                                    | -           | 否       |

#### `protocol: dns`（传统 DNS 端点条目）

| 参数     | 类型   | 描述                                                                           | 默认值 | 是否必填 |
| :------- | :----- | :----------------------------------------------------------------------------- | :----- | :------- |
| `addr`   | 字符串 | DNS 服务器地址，格式为 `IP:端口`（例如 `223.5.5.5:53` 或 `192.168.1.53:53`）。 | -      | **是**   |
| `weight` | 整数   | (可选) 端点的权重，仅在组的 `policy` 为 `weighted` 时生效。                    | `1`    | 否       |

<a id="auth-认证-参数详解"></a>

### `auth` (认证) 参数详解

| 参数       | 类型   | 描述                                                                       | 默认值 | 是否必填                       |
| :--------- | :----- | :------------------------------------------------------------------------- | :----- | :----------------------------- |
| `type`     | 字符串 | 认证类型。可选值为 `basic` (HTTP 基本认证) 或 `bearer` (Bearer 令牌认证)。 | -      | **是** (若 `auth` 块存在)      |
| `username` | 字符串 | 用户名，仅在 `type` 为 `basic` 时使用。                                    | -      | **是** (若 `type` 为 `basic`)  |
| `password` | 字符串 | 密码，仅在 `type` 为 `basic` 时使用。                                      | -      | **是** (若 `type` 为 `basic`)  |
| `token`    | 字符串 | Bearer 令牌，仅在 `type` 为 `bearer` 时使用。                              | -      | **是** (若 `type` 为 `bearer`) |

---

<a id="全局重试策略-retry"></a>

### `retry`（上游组级别重试策略）

`retry` 位于**上游组内部**（与 `endpoints`、`proxy` 同级），用于控制向该组上游发起请求失败时的重试行为。

> **注意**：`rules.remote` 下载规则文件时使用其自身的 `retry` 配置（见路由规则配置章节），不受此处影响。

```yaml
upstreams:
    - name: "google_public"
      policy: "random"
      endpoints:
          - url: "https://dns.google/dns-query"
      retry:
          attempts: 3
          delay: 2
```

| 参数       | 类型 | 描述                                                                               | 默认值 | 是否必填                    |
| :--------- | :--- | :--------------------------------------------------------------------------------- | :----- | :-------------------------- |
| `attempts` | 整数 | 最大重试次数（包含第一次请求），有效范围 `1-100`。例如 `3` 表示总共最多尝试 3 次。 | -      | **是**（若 `retry` 块存在） |
| `delay`    | 整数 | 退避基准延迟（秒），有效范围 `1-120`。实际重试间隔会随退避策略增长。               | -      | **是**（若 `retry` 块存在） |

---

### 场景化配置示例

#### 场景一：基础隐私设置

**目标**：使用多个公共 DoH 解析服务，通过随机策略增强隐私性。

```yaml
upstreams:
    - name: "privacy_first"
      protocol: "doh"
      policy: "random"
      endpoints:
          - url: "https://cloudflare-dns.com/dns-query"
            method: "get"
          - url: "https://dns.google/dns-query"
          - url: "https://doh.opendns.com/dns-query"
```

#### 场景二：高性能主备模式

**目标**：主要使用一个高性能的服务器，当它不可用时，自动切换到备用服务器。

```yaml
upstreams:
    - name: "high_perf"
      protocol: "doh"
      policy: "weighted"
      endpoints:
          - url: "https://fast-doh.com/dns-query"
            weight: 90 # 90% 的请求会发往这里
          - url: "https://backup-doh.com/dns-query"
            weight: 10 # 10% 的请求会发往这里，作为探活和备用
```

#### 场景三：连接需要认证和代理的私有 DoH 服务

**目标**：连接到类似 NextDNS 这样的私有 DoH 服务，它可能需要认证，并且你希望通过代理访问它。

```yaml
upstreams:
    - name: "private_nextdns"
      protocol: "doh"
      policy: "roundrobin" # 如果只有一个端点，策略无所谓
      # 为整个组配置代理
      proxy: "socks5://127.0.0.1:1080"
      endpoints:
          - url: "https://dns.nextdns.io/YOUR_CONFIG_ID"
            # 也可以在这里为单个服务器配置认证
            # auth:
            #   type: "bearer"
            #   token: "YOUR_API_KEY"
```

#### 场景四：转发到传统 DNS 上游（UDP/TCP，端口 53）

**目标**：将特定流量转发到局域网 DNS（如 `dnsmasq`/路由器 DNS/CoreDNS）或公共传统 DNS，避免额外的 DoH 封装。

```yaml
dns:
    prefer_tcp: false # 默认 UDP，遇到 TC=1 再回退 TCP

upstreams:
    - name: "lan_dns"
      protocol: "dns"
      policy: "roundrobin"
      endpoints:
          - addr: 192.168.1.53:53
          - addr: 192.168.1.1:53
```

---

### 下一步

- [➡️ 回顾上游的核心概念](../concepts/upstream.md)
- [➡️ 配置路由规则](./routing-rules.md)
- [➡️ 返回配置总览](./index.md)
