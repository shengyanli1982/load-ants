# 上游组配置

`upstream_groups` 是 Load Ants 配置中至关重要的一环。它定义了你的 DNS 查询最终将被发送到哪些 DoH 服务器，以及如何在高可用、高性能和高隐私之间取得平衡。

本配置项是一个**列表**，这意味着你可以定义多个独立配置的"上游组"。

想要深入了解上游管理和负载均衡策略背后的理念吗？请阅读 [**核心概念：上游服务器管理**](../concepts/upstream.md)。

### 顶级参数 (上游组级别)

#### 示例

```yaml
upstream_groups:
    - name: "google_public"
      strategy: "random"
      servers:
          # ... server 列表在这里定义 ...
      proxy: "http://127.0.0.1:7890"
```

#### 参数详解

| 参数       | 类型   | 描述                                                                                                                                                                                            | 默认值 | 是否必填 |
| :--------- | :----- | :---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :----- | :------- |
| `name`     | 字符串 | 为这个上游组指定一个唯一的名称。这个名称将在路由规则 (`static_rules` 或 `remote_rules`) 的 `target` 字段中被引用。                                                                              | -      | **是**   |
| `strategy` | 字符串 | 定义该组内服务器的负载均衡策略。可选值为：`roundrobin` (轮询), `weighted` (加权轮询), `random` (随机)。                                                                                         | -      | **是**   |
| `servers`  | 列表   | 定义本组包含的一个或多个 DoH 服务器。详见下方的 `servers` 参数详解。                                                                                                                            | -      | **是**   |
| `proxy`    | 字符串 | (可选) 为**整个组**的服务器指定一个出站代理。所有发往该组内服务器的 DoH 请求都将通过此代理。支持 `http`, `https` 和 `socks5` 协议。例如: `http://user:pass@host:port` 或 `socks5://host:port`。 | -      | 否       |

---

### `servers` 参数详解

`servers` 是一个列表，列表中的每个对象都代表一个 DoH 服务器。

| 参数           | 类型   | 描述                                                                                                                                                                   | 默认值      | 是否必填 |
| :------------- | :----- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :---------- | :------- |
| `url`          | 字符串 | DoH 服务器的完整 URL。                                                                                                                                                 | -           | **是**   |
| `weight`       | 整数   | (可选) 服务器的权重，仅在组的 `strategy` 为 `weighted` 时生效。权重越高的服务器，被选中的概率就越大。                                                                  | `1`         | 否       |
| `method`       | 字符串 | (可选) 与该服务器通信时使用的 HTTP 方法。可选值为 `get` 或 `post`。                                                                                                    | `"post"`    | 否       |
| `content_type` | 字符串 | (可选) DoH 请求的内容类型。可选值为 `message` (对应 `application/dns-message`) 或 `json` (对应 `application/dns-json`)。注意：如果设为 `json`，`method` 必须为 `get`。 | `"message"` | 否       |
| `auth`         | 对象   | (可选) 访问此特定服务器所需的认证配置。详见下方的 `auth` 参数详解。                                                                                                    | -           | 否       |

### `auth` (认证) 参数详解

| 参数       | 类型   | 描述                                                                       | 默认值 | 是否必填                       |
| :--------- | :----- | :------------------------------------------------------------------------- | :----- | :----------------------------- |
| `type`     | 字符串 | 认证类型。可选值为 `basic` (HTTP 基本认证) 或 `bearer` (Bearer 令牌认证)。 | -      | **是** (若 `auth` 块存在)      |
| `username` | 字符串 | 用户名，仅在 `type` 为 `basic` 时使用。                                    | -      | **是** (若 `type` 为 `basic`)  |
| `password` | 字符串 | 密码，仅在 `type` 为 `basic` 时使用。                                      | -      | **是** (若 `type` 为 `basic`)  |
| `token`    | 字符串 | Bearer 令牌，仅在 `type` 为 `bearer` 时使用。                              | -      | **是** (若 `type` 为 `bearer`) |

---

### 全局重试策略 (`retry`)

此配置块定义了当向上游服务器发出的请求失败时的全局重试行为。它位于配置文件的顶层，与 `upstream_groups` 平级。

```yaml
# 在 config.yaml 的顶层
retry:
    attempts: 3
    delay: 1
```

| 参数       | 类型 | 描述                                                                            | 默认值 | 是否必填 |
| :--------- | :--- | :------------------------------------------------------------------------------ | :----- | :------- |
| `attempts` | 整数 | 当请求失败时，最多尝试的次数（包括第一次请求）。例如，`3` 表示总共会尝试 3 次。 | `3`    | 否       |
| `delay`    | 整数 | 每次重试之间的初始延迟时间（秒）。后续的重试延迟可能会增加。                    | `1`    | 否       |

---

### 场景化配置示例

#### 场景一：基础隐私设置

**目标**：使用多个公共 DoH 解析服务，通过随机策略增强隐私性。

```yaml
upstream_groups:
    - name: "privacy_first"
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
