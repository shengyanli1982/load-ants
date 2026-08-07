# 地理封锁解除

本教程演示如何利用 Load Ants 的路由功能，为特定在线服务（如 Netflix、Hulu）配置专用的 DNS 解析路径，以访问特定区域的内容库：这些服务的 DNS 查询通过代理发送到上游服务器，其余常规流量仍通过直连路径解析。

### 目标

- 创建两个上游组：一个用于常规流量（直连），另一个用于需要特殊处理的流量（通过代理）。
- 使用正则表达式（`regex`）匹配精确识别目标服务的域名。
- 将匹配到的 DNS 查询路由到“代理组”，其他所有查询路由到“直连组”。

### 先决条件

1.  一台可以运行 Load Ants 的主机。
2.  一个可以正常工作的 HTTP 代理（Load Ants 仅支持 http(s) 协议的代理，不支持 SOCKS5）。你需要该代理的 IP 地址、端口，若代理要求认证，还需要用户名和密码。本例假设代理地址为 `http://proxy.example.com:8888`。
3.  对 Load Ants 的[上游组](../configuration/upstream-groups.md)和[路由规则](../configuration/routing-rules.md)有基本了解。

### 步骤一：`config.yaml` 配置

本配方的全部逻辑都在配置文件中实现：组合上游组与路由规则。

```yaml
# ----------------------------------
# 日志和服务端配置（省略）
# ...
# ----------------------------------

# ----------------------------------
# 上游组
# ----------------------------------
upstream_groups:
    # 1. 直连组：用于所有常规流量
    - name: "direct_group"
      scheme: "doh"
      strategy: "random"
      servers:
          - url: "https://dns.google/dns-query" # 你可以选择任何喜欢的公共 DNS

    # 2. 代理组：用于流媒体服务
    - name: "streaming_proxy_group"
      scheme: "doh"
      strategy: "random"
      # 关键：为此组配置代理
      proxy: "http://proxy.example.com:8888"
      servers:
          # 选择与代理服务器地理位置相近的 DNS 服务
          - url: "https://cloudflare-dns.com/dns-query"

# ----------------------------------
# 路由规则
# ----------------------------------
static_rules:
    # 1. 规则A：匹配 Netflix 相关域名
    # 使用正则表达式匹配 Netflix 的主域名和其 CDN 域名
    - match: "regex"
      patterns:
          - "^(.*\\.)?netflix\\.com$"
          - "^(.*\\.)?nflxvideo\\.net$"
      action: "forward"
      target: "streaming_proxy_group"

    # 2. 规则B：匹配 Hulu 相关域名
    - match: "regex"
      patterns:
          - "^(.*\\.)?hulu\\.com$"
      action: "forward"
      target: "streaming_proxy_group"

    # 3. 规则C：默认规则（Fallback）
    # 确保所有其他流量都走直连组
    - match: "wildcard"
      patterns: ["*"]
      action: "forward"
      target: "direct_group"
```

**配置逻辑解读**：

1.  **`upstream_groups`**：
    - `direct_group`：一个常规上游组，不走任何代理。
    - `streaming_proxy_group`：这个组的特殊之处在于配置了 `proxy` 字段。所有通过该组转发的 DNS 查询，其网络流量都会经过 `http://proxy.example.com:8888`。

    > 提示：`proxy` 仅接受 `http`/`https` 协议的代理地址，且仅适用于 `scheme: doh` 的上游组；传统 DNS 上游（`scheme: dns`）不支持代理。

2.  **`static_rules`**：
    - Load Ants 的路由遵循“拦截优先（block first）”与“分层匹配”的原则：匹配优先级为 `exact` > `wildcard` > `regex` > `*`（全局通配符）。其中 `exact` 阶段合并查找 `block` 与 `forward` 规则（同一 `pattern` 同时存在两者时 `block` 胜出），其余阶段均先查 `block` 规则、再查 `forward` 规则。
    - **规则 A 和 B**：使用 `regex` 捕获目标服务的域名。例如，`^(.*\\.)?netflix\\.com$` 可以匹配 `netflix.com` 及其所有子域名（如 `www.netflix.com`、`movies.prod.netflix.com`）。当 DNS 查询匹配到这些模式时，Load Ants 会执行 `forward` 动作，并把查询交给 `target` 指定的 `streaming_proxy_group`。
    - **规则 C**：`wildcard` 类型的 `"*"` 匹配所有域名，但它的优先级最低。因此，只有当一个查询**没有**匹配到任何 `regex` 规则时，才会落到这个“全匹配”规则上，并被转发到 `direct_group`。

### 步骤二：启动和验证

1.  **启动 Load Ants**：
    使用你偏好的方式启动 Load Ants（直接运行二进制文件，或通过 Docker/systemd）。

    ```bash
    ./loadants -c /path/to/your/config.yaml
    ```

2.  **验证**：
    使用 `dig` 或 `nslookup`。
    - **测试一个常规域名**：

        ```bash
        dig @localhost www.github.com
        ```

        预期看到正常的解析结果。在 Load Ants 的 `debug` 日志中，可以看到该查询被转发到了 `direct_group`。

    - **测试一个目标服务域名**：
        ```bash
        dig @localhost www.netflix.com
        ```
        预期同样看到正常的解析结果。查看 Load Ants 的 `debug` 日志，可以发现这次查询被路由到了 `streaming_proxy_group`，且日志会显示通过代理向上游服务器发出了请求。

### 结论

通过这种路由策略，你可以按域名将 DNS 查询分发到不同的网络路径。在此基础上，还可以添加更多上游组（例如对应不同国家或地区的代理）与路由规则，实现更复杂的流量调度。

---

### 下一步

- [➡️ 回顾路由规则配置](../configuration/routing-rules.md)
- [➡️ 尝试其他实例](./ad-blocking.md)
- [➡️ 返回实例总览](./index.md)
