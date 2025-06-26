# 家庭实验室 DNS

对于拥有家庭实验室 (Homelab) 或小型本地网络的用户来说，记住一堆 IP 地址（例如 `192.168.1.10` 用于 NAS，`192.168.1.20` 用于 Plex）是一件很麻烦的事。本配方将向你展示如何利用 Load Ants 最简单的 `static_rules` 功能，为你本地网络上的设备赋予友好的、易于记忆的域名（如 `nas.lan`, `plex.lan`）。

### 目标

-   将自定义的本地域名解析到其对应的内部 IP 地址。
-   拦截对这些本地域名的 AAAA (IPv6) 请求，以避免某些客户端出现延迟。
-   所有其他正常的互联网域名查询则转发给常规的上游 DNS 服务器。

### 先决条件

1.  一台可以运行 Load Ants 的主机。
2.  知道你想要映射的本地设备的静态 IP 地址。

### 步骤一：`config.yaml` 配置

这个配方的配置非常简单，并且可以轻松地与本 Cookbook 中的其他配方（如广告拦截）组合使用。

```yaml
# ----------------------------------
# 日志、服务监听、缓存等配置 (省略)
# ...
# ----------------------------------

# ----------------------------------
# 上游服务器组
# ----------------------------------
upstream_groups:
    - name: "public_dns"
      strategy: "random"
      servers:
          - url: "https://dns.google/dns-query"

# ----------------------------------
# 路由规则
# ----------------------------------
static_rules:
    # --- 本地域名解析 ---
    # 将 nas.lan 解析到 192.168.1.10
    - match: "exact"
      patterns: ["nas.lan"]
      action: "resolve"
      target: "192.168.1.10"

    # 将 plex.lan 解析到 192.168.1.20
    - match: "exact"
      patterns: ["plex.lan"]
      action: "resolve"
      target: "192.168.1.20"

    # --- 拦截本地名的 IPv6 查询 ---
    # 避免客户端在请求 A 记录后，又徒劳地等待一个不存在的 AAAA 记录的响应
    - match: "regex"
      patterns: ["^.*\\.lan$"] # 匹配所有以 .lan 结尾的域名
      query_type: ["AAAA"]
      action: "block"

    # --- 默认规则 ---
    # 将所有其他查询转发到公共 DNS
    - match: "wildcard"
      patterns: ["*"]
      action: "forward"
      target: "public_dns"
```

**配置逻辑解读**:

1.  **`action: "resolve"`**: 这是我们第一次使用这个特殊的动作。当 `action` 被设置为 `resolve` 时，`target` 字段不再是一个上游组的名称，而是一个 **IP 地址**。Load Ants 会立即以此 IP 地址作为响应，而不会向上游发出任何查询。这是实现自定义 DNS 记录的核心。

2.  **`match: "exact"`**: 我们为每一个本地域名使用 `exact` 匹配，这是最高效、最精确的方式。

3.  **拦截 AAAA 请求**: 这是一个重要的优化。很多现代操作系统和浏览器会默认同时请求一个域名的 A (IPv4) 和 AAAA (IPv6) 记录（这种行为被称为 "Happy Eyeballs"）。对于我们只拥有 IPv4 地址的本地域名，如果不处理 AAAA 请求，客户端可能会因为等待一个永远不会到来的 AAAA 响应而产生微小的延迟。因此，我们添加了一条 `regex` 规则，它匹配所有以 `.lan` 结尾的域名的 `AAAA` 类型查询，并直接 `block` (拦截) 它们，从而立即告知客户端此域名没有 IPv6 地址。

4.  **`query_type`**: 在拦截 AAAA 的规则中，我们使用了 `query_type` 字段来限定此条规则只对特定类型的 DNS 查询生效。

5.  **默认规则**: 和其他配方一样，一个 `wildcard` 规则作为"接球手"，确保所有非本地域名的查询都能被正常地转发出去。

### 步骤二：启动和验证

1.  **启动 Load Ants** 并将你的客户端 DNS 指向它。

2.  **验证**:
    使用 `dig` 或 `nslookup`。

    -   **测试一个本地域名**:

        ```bash
        dig @localhost nas.lan
        ```

        你应该能立即收到 `192.168.1.10` 这个 A 记录的响应。

    -   **测试本地域名的 AAAA 记录**:

        ```bash
        dig @localhost AAAA nas.lan
        ```

        你应该会收到一个 `NXDOMAIN` 或 `0.0.0.0` 的响应，表示查询被成功拦截。

    -   **测试一个公共域名**:
        ```bash
        dig @localhost www.google.com
        ```
        你应该会收到一个正常的、由上游 `public_dns` 服务器返回的响应。

### 结论

通过 `resolve` 动作，Load Ants 不仅是一个 DNS 代理和转发器，更可以成为一个轻量级的权威 DNS 服务器，让你能够完全掌控自己网络内的域名解析，极大地提升了家庭网络的便利性。

---

### 下一步

-   [➡️ 回顾路由规则配置](../configuration/routing-rules.md)
-   [➡️ 尝试其他实例](./ad-blocking.md)
-   [➡️ 返回实例总览](./index.md)
