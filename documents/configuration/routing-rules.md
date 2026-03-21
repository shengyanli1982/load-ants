# 路由规则配置

路由规则是 Load Ants 的"大脑"，它决定了如何处理每一个接收到的 DNS 查询。通过组合使用静态规则 (`static_rules`) 和远程规则 (`remote_rules`)，你可以构建出强大而灵活的 DNS 流量管理策略。

想要深入了解路由的工作原理和匹配优先级吗？请阅读 [**核心概念：智能路由机制**](../concepts/routing.md)。

### `static_rules` (静态规则)

静态规则是在 `config.yaml` 文件中手动定义的规则，适用于那些不经常变化的、固定的路由策略。

#### 示例

```yaml
static_rules:
    - match: "exact"
      patterns: ["ads.example.com"]
      action: "block"

    - match: "wildcard"
      patterns: ["*.my-company.internal"]
      action: "forward"
      target: "internal_dns_group"
```

#### 参数详解

| 参数       | 类型   | 描述                                                                                                                 | 默认值 | 是否必填                          |
| :--------- | :----- | :------------------------------------------------------------------------------------------------------------------- | :----- | :-------------------------------- |
| `match`    | 字符串 | 匹配类型。可选值为 `exact` (精确), `wildcard` (通配符), `regex` (正则)。                                             | -      | **是**                            |
| `patterns` | 列表   | 匹配模式的列表。根据 `match` 类型的不同，这里的模式格式也不同。                                                      | -      | **是**                            |
| `action`   | 字符串 | 当匹配成功时执行的动作。可选值为 `block` (拦截) 或 `forward` (转发)。                                                | -      | **是**                            |
| `target`   | 字符串 | 目标上游组的名称。仅在 `action` 为 `forward` 时需要。此名称必须与 `upstream_groups` 中定义的某个组的 `name` 相对应。 | -      | **是** (若 `action` 为 `forward`) |

#### `patterns` 格式与校验规则（重要）

- `patterns` 必须为非空列表（至少包含 1 个模式）。
- 当 `match: "exact"`：每个模式是一个完整域名字符串；匹配时不区分大小写；允许末尾带 `.`（会被归一化处理）。
- 当 `match: "wildcard"`：每个模式必须是 `*` 或 `*.domain.tld` 的形式；同样允许末尾带 `.`。
- 当 `match: "regex"`：每个模式必须是一个合法的正则表达式（非法正则会导致加载配置失败）；同一动作内完全相同的 regex pattern 会在启动期直接报 `RuleConflict`。
- 当 `action: "forward"`：必须提供 `target`，且该值必须引用一个已存在的 `upstream_groups[].name`；另外，上游组名称必须唯一。

---

### `remote_rules` (远程规则)

远程规则允许你从一个 URL 动态加载和应用规则列表。这对于订阅由社区维护的、经常更新的列表（如广告拦截、恶意网站列表）非常有用。

与上游组 (`upstream_groups`) 不同，`remote_rules` 拥有自己独立的网络配置，用于控制规则文件的下载行为。

#### 示例

```yaml
remote_rules:
    - type: "url"
      url: "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/reject-list.txt"
      format: "v2ray"
      action: "block"
      proxy: "http://127.0.0.1:7890"
```

#### 参数详解

| 参数       | 类型   | 描述                                                                                                                               | 默认值            | 是否必填                          |
| :--------- | :----- | :--------------------------------------------------------------------------------------------------------------------------------- | :---------------- | :-------------------------------- |
| `type`     | 字符串 | 规则类型。目前仅支持 `url`。                                                                                                       | `"url"`           | **是**                            |
| `url`      | 字符串 | 远程规则文件的 URL。                                                                                                               | -                 | **是**                            |
| `format`   | 字符串 | 规则文件的格式。目前仅支持 `v2ray`。                                                                                               | `"v2ray"`         | **是**                            |
| `action`   | 字符串 | 应用于此列表中所有域名的动作。可选值为 `block` 或 `forward`。                                                                      | -                 | **是**                            |
| `target`   | 字符串 | 目标上游组的名称。当 `action` 为 `forward` 时必填。                                                                                | -                 | **是** (若 `action` 为 `forward`) |
| `failure_policy` | 字符串 | 启动时远程源加载失败的处理方式。可选值为 `strict` 或 `lenient`；`strict` 在没有有效快照恢复时会阻断启动。                              | `"strict"`        | 否                                |
| `proxy`    | 字符串 | (可选) 获取此规则文件时使用的 HTTP/SOCKS5 代理。                                                                                   | -                 | 否                                |
| `auth`     | 对象   | (可选) 访问此规则文件 URL 所需的认证配置。结构与[上游组的 `auth` 配置](./upstream-groups.md#auth-认证-参数详解)相同。              | -                 | 否                                |
| `retry`    | 对象   | (可选) 获取此规则文件时的网络重试策略。结构与[全局重试策略](./upstream-groups.md#全局重试策略-retry)相同，但此为该规则独享的配置。 | -                 | 否                                |
| `max_size` | 整数   | (可选) 允许下载的远程规则文件的最大体积（字节）。                                                                                  | `10485760` (10MB) | 否                                |

> **提示**：
>
> - `max_size` 的有效范围为 `1024`（1KB）到 `52428800`（50MB）。
> - `remote_rules` 的下载会复用全局 `http_client` 的超时/连接池配置，但其 `proxy`/`auth`/`retry` 为规则源独享配置。
> - 当启用 `remote_rule_snapshot` 且对应 URL 已存在有效快照时，远程源失败会优先尝试使用 last-known-good 快照恢复；此时仍会记录 degraded/fallback 日志。

> ✨ **专家提示**:
>
> **独立的网络配置**：`remote_rules` 拥有独立的 `proxy`, `auth`, `retry` 配置，是因为规则文件所在的服务器网络环境可能与你日常使用的 DoH 服务器完全不同。例如，某个规则列表可能托管在需要特定代理才能访问的 GitHub Gist 上，而你的 DoH 查询则希望直连。这种独立性提供了极大的灵活性。

### `remote_rule_snapshot` (远程规则快照)

`remote_rule_snapshot` 是一个顶层全局配置，用来控制 remote rule 的本地 last-known-good 持久化。它不改变远程规则的匹配类型，只负责在启动时保存和恢复“上一份成功加载过的规则集”。

#### 示例

```yaml
remote_rule_snapshot:
  enabled: true
  path: ".load-ants/remote-rule-snapshots"
```

#### 参数详解

| 参数 | 类型 | 描述 | 默认值 | 是否必填 |
| :--- | :--- | :--- | :--- | :--- |
| `enabled` | 布尔 | 是否启用远程规则快照读写与 last-known-good 恢复。 | `true` | 否 |
| `path` | 字符串 | 快照目录。每个远程 URL 会派生为目录下的一个独立文件。 | `".load-ants/remote-rule-snapshots"` | 否 |

#### 行为说明

- 远程源成功加载后，Load Ants 会把解析后的规则写入该 URL 对应的快照文件。
- 启动时若远程源失败，但存在有效快照，则会使用该快照恢复，并把该源标记为 degraded/fallback。
- 若远程源失败且没有有效快照，则行为仍回到 `failure_policy`：
  - `strict`：阻断启动。
  - `lenient`：允许降级启动，但只使用其他成功源和静态规则。

---

### 配置配方：实用场景示例

#### 配方一：全面的广告和追踪器拦截

**目标**：结合本地规则和远程列表，打造一个强大的广告拦截系统。

```yaml
static_rules:
    # 1. 手动屏蔽一些顽固的、或远程列表中没有的域名
    - match: "exact"
      patterns:
          - "specific-ad-server.com"
          - "annoying-tracker.net"
      action: "block"

    # 2. 默认将所有流量转发到干净的上游
    - match: "wildcard"
      patterns: ["*"]
      action: "forward"
      target: "privacy_first" # 假设你已定义了一个注重隐私的上游组

remote_rules:
    # 3. 订阅一个由社区维护的广告域名列表
    - type: "url"
      url: "https://raw.githubusercontent.com/privacy-respecting-software/Blocky-Adlists/main/dns-hole-list.txt" # 这是一个示例列表，你可以替换为任何兼容的列表
      format: "v2ray"
      failure_policy: "strict"
      action: "block"
```

**工作原理**：Load Ants 会先判断所有 `block` 规则，再判断 `forward` 规则；在同一动作内，优先级固定为 `exact` > `wildcard` > `regex` > `*`。因此，这里的远程列表和静态 `block` 规则都会先参与 `block` 阶段匹配；如果都没有命中，查询才会继续落到最后的全局通配符 `forward` 规则，被转发到干净的上游。如果该远程列表在某次启动时失败，而本地存在有效 snapshot，则仍可以使用 last-known-good 恢复。

#### 配方二：为特定服务配置专用代理

**目标**：只让特定服务（如 Netflix）的 DNS 查询通过代理，其他所有查询直连。

```yaml
# 注意：以下 upstream_groups 应在你的主配置文件的顶层定义。
# 这里为了示例清晰而放在一起。
# 更多详情请查阅 [上游组配置](./upstream-groups.md)。
upstream_groups:
  - name: "direct_group"
    strategy: "random"
    servers:
      - url: "https://dns.google/dns-query"
  - name: "proxy_group"
    strategy: "random"
    proxy: "socks5://127.0.0.1:1080" # 你的代理地址
    servers:
      - url: "https://cloudflare-dns.com/dns-query"

static_rules:
  # 1. 将 Netflix 相关域名转发到"代理组"
  - match: "regex"
    patterns: ["^(.*\.)?netflix\.com$", "^(.*\.)?nflxvideo\.net$"]
    action: "forward"
    target: "proxy_group"

  # 2. 其他所有域名都转发到"直连组"
  - match: "wildcard"
    patterns: ["*"]
    action: "forward"
    target: "direct_group"
```

**工作原理**：由于 `regex` 匹配的优先级高于 `wildcard`，符合 Netflix 模式的查询会在 `forward` 阶段先命中 `regex` 规则，并被转发到 `proxy_group`。其他查询则会继续落到全局 `wildcard` 规则，走向 `direct_group`。这里依赖的是固定优先级，而不是配置项的声明顺序。

---

### 下一步

- [➡️ 回顾智能路由的核心概念](../concepts/routing.md)
- [➡️ 了解上游组配置](./upstream-groups.md)
- [➡️ 返回配置总览](./index.md)
