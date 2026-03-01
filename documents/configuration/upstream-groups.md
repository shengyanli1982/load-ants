# 上游组配置

`upstream_groups` 是 Load Ants 配置中至关重要的一环。它定义了你的 DNS 查询最终将被发送到哪些上游解析器（**DoH** 或 **传统 DNS（UDP/TCP）**），以及如何在高可用、高性能和高隐私之间取得平衡。

本配置项是一个**列表**，这意味着你可以定义多个独立配置的"上游组"。

想要深入了解上游管理和负载均衡策略背后的理念吗？请阅读 [**核心概念：上游服务器管理**](../concepts/upstream.md)。

### 顶级参数 (上游组级别)

#### 示例

```yaml
upstream_groups:
    - name: "google_public"
      scheme: "doh"
      strategy: "random"
      servers:
          # ... server 列表在这里定义 ...
      retry:
          attempts: 3
          delay: 2
      proxy: "http://127.0.0.1:7890"
```

#### 参数详解

| 参数       | 类型   | 描述                                                                                                                                                                                            | 默认值 | 是否必填 |
| :--------- | :----- | :---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :----- | :------- |
| `name`     | 字符串 | 为这个上游组指定一个唯一的名称。这个名称将在路由规则 (`static_rules` 或 `remote_rules`) 的 `target` 字段中被引用。                                                                              | -      | **是**   |
| `scheme`   | 字符串 | 上游组类型。可选值：`doh`（DoH 上游）或 `dns`（传统 DNS 上游，UDP/TCP）。为兼容旧配置，也支持别名字段 `protocol`。                                                                              | `doh`  | 否       |
| `strategy` | 字符串 | 定义该组内服务器的负载均衡策略。可选值为：`roundrobin` (轮询), `weighted` (加权轮询), `random` (随机)。                                                                                         | -      | **是**   |
| `servers`  | 列表   | 定义本组包含的一个或多个上游服务器。其条目结构取决于 `scheme`（DoH 使用 `url`，DNS 使用 `addr`）。详见下方的 `servers` 参数详解。                                                              | -      | **是**   |
| `retry`    | 对象   | (可选，仅 `scheme: doh`) 本组上游请求的重试策略。详见下方 `retry` 参数详解。                                                                                                                    | -      | 否       |
| `proxy`    | 字符串 | (可选，仅 `scheme: doh`) 为**整个组**的服务器指定一个出站代理。所有发往该组内服务器的 DoH 请求都将通过此代理。支持 `http`, `https` 和 `socks5` 协议。例如: `http://user:pass@host:port` 或 `socks5://host:port`。 | -      | 否       |

---

### `servers` 参数详解

`servers` 是一个列表，列表中的每个对象都代表一个上游服务器。其条目结构由 `scheme` 决定：

- `scheme: doh`：条目使用 `url`（DoH 服务器 URL）。
- `scheme: dns`：条目使用 `addr`（传统 DNS 的 `IP:端口`，通常为 `:53`）。

> 说明：
>
> - 如果你不填写 `scheme`，默认按 `doh` 处理（与旧版本行为一致）。
> - 若 `scheme: dns`，该组不支持 `proxy` 与 `retry`（会触发配置校验错误）。

#### `scheme: doh`（DoH 服务器条目）

| 参数           | 类型   | 描述                                                                                                                                                                   | 默认值      | 是否必填 |
| :------------- | :----- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :---------- | :------- |
| `url`          | 字符串 | DoH 服务器的完整 URL。                                                                                                                                                 | -           | **是**   |
| `weight`       | 整数   | (可选) 服务器的权重，仅在组的 `strategy` 为 `weighted` 时生效。权重越高的服务器，被选中的概率就越大。                                                                  | `1`         | 否       |
| `method`       | 字符串 | (可选) 与该服务器通信时使用的 HTTP 方法。可选值为 `get` 或 `post`。                                                                                                    | `"post"`    | 否       |
| `content_type` | 字符串 | (可选) DoH 请求的内容类型。可选值为 `message` (对应 `application/dns-message`) 或 `json` (对应 `application/dns-json`)。注意：如果设为 `json`，`method` 必须为 `get`。 | `"message"` | 否       |
| `auth`         | 对象   | (可选) 访问此特定服务器所需的认证配置。详见下方的 `auth` 参数详解。                                                                                                    | -           | 否       |

#### `scheme: dns`（传统 DNS 服务器条目）

| 参数     | 类型   | 描述                                                                                          | 默认值 | 是否必填 |
| :------- | :----- | :-------------------------------------------------------------------------------------------- | :----- | :------- |
| `addr`   | 字符串 | DNS 服务器地址，格式为 `IP:端口`（例如 `223.5.5.5:53` 或 `192.168.1.53:53`）。                | -      | **是**   |
| `weight` | 整数   | (可选) 服务器的权重，仅在组的 `strategy` 为 `weighted` 时生效。                               | `1`    | 否       |

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

`retry` 位于**上游组内部**（与 `servers`、`proxy` 同级），用于控制向该组上游发起请求失败时的重试行为。

> **注意**：`remote_rules` 下载规则文件时使用其自身的 `retry` 配置（见路由规则配置章节），不受此处影响。

```yaml
upstream_groups:
    - name: "google_public"
      strategy: "random"
      servers:
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
upstream_groups:
    - name: "privacy_first"
      scheme: "doh"
      strategy: "random"
      servers:
          - url: "https://cloudflare-dns.com/dns-query"
            method: "get"
          - url: "https://dns.google/dns-query"
          - url: "https://doh.opendns.com/dns-query"
```

#### 场景二：高性能主备模式

**目标**：主要使用一个高性能的服务器，当它不可用时，自动切换到备用服务器。

```yaml
upstream_groups:
    - name: "high_perf"
      scheme: "doh"
      strategy: "weighted"
      servers:
          - url: "https://fast-doh.com/dns-query"
            weight: 90 # 90% 的请求会发往这里
          - url: "https://backup-doh.com/dns-query"
            weight: 10 # 10% 的请求会发往这里，作为探活和备用
```

#### 场景三：连接需要认证和代理的私有 DoH 服务

**目标**：连接到类似 NextDNS 这样的私有 DoH 服务，它可能需要认证，并且你希望通过代理访问它。

```yaml
upstream_groups:
    - name: "private_nextdns"
      scheme: "doh"
      strategy: "roundrobin" # 如果只有一个服务器，策略无所谓
      # 为整个组配置代理
      proxy: "socks5://127.0.0.1:1080"
      servers:
          - url: "https://dns.nextdns.io/YOUR_CONFIG_ID"
            # 也可以在这里为单个服务器配置认证
            # auth:
            #   type: "bearer"
            #   token: "YOUR_API_KEY"
```

#### 场景四：转发到传统 DNS 上游（UDP/TCP，端口 53）

**目标**：将特定流量转发到局域网 DNS（如 `dnsmasq`/路由器 DNS/CoreDNS）或公共传统 DNS，避免额外的 DoH 封装。

```yaml
dns_client:
    prefer_tcp: false # 默认 UDP，遇到 TC=1 再回退 TCP

upstream_groups:
    - name: "lan_dns"
      scheme: "dns"
      strategy: "roundrobin"
      servers:
          - addr: 192.168.1.53:53
          - addr: 192.168.1.1:53
```

---

### 下一步

- [➡️ 回顾上游的核心概念](../concepts/upstream.md)
- [➡️ 配置路由规则](./routing-rules.md)
- [➡️ 返回配置总览](./index.md)
