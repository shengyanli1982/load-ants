# 家庭实验室 DNS

对于拥有家庭实验室 (Homelab) 或小型本地网络的用户来说，记住一堆 IP 地址（例如 `192.168.1.10` 用于 NAS，`192.168.1.20` 用于 Plex）是一件很麻烦的事。本配方将向你展示如何利用 Load Ants 的路由功能，结合一个轻量级的本地 DNS 服务器，为你本地网络上的设备赋予友好的、易于记忆的域名（如 `nas.lan`, `plex.lan`）。

### 目标

-   将对自定义本地域名（`.lan` 后缀）的 DNS 查询，转发给一个专门的本地 DNS 服务器进行解析。
-   所有其他正常的互联网域名查询，则通过 Load Ants 转发给常规的上游 DoH 服务器。

### 架构变更解释

Load Ants 的核心功能是 **DNS-over-HTTPS (DoH) 代理**，它本身不直接从 IP 地址生成 DNS 响应。它的 `static_rules` 只能决定将查询 `forward` (转发) 到哪个上游组，或直接 `block` (拦截)。

因此，为了实现本地域名解析，我们需要一个能够处理这种请求的"上游"。最简单的方法就是在你的网络中运行一个传统的 DNS 服务器（如 `dnsmasq`, `CoreDNS`, 或路由器自带的 DNS 服务），并让 Load Ants 将特定查询指向它。

### 先决条件

1.  一台可以运行 Load Ants 的主机。
2.  一个在你的局域网中运行的、**传统的 DNS 服务器**。在本例中，我们假设这个服务器的地址是 `192.168.1.53:53`。你已经在这个服务器上配置好了 `nas.lan` -> `192.168.1.10` 的解析。
3.  对 Load Ants 的[上游组](../configuration/upstream-groups.md)和[路由规则](../configuration/routing-rules.md)有基本了解。

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
    # 1. 公共 DNS 组：用于所有常规的互联网查询
    - name: "public_dns"
      strategy: "random"
      servers:
          - url: "https://dns.google/dns-query"

    # 2. 本地 DNS 组：专门用于解析内部域名
    - name: "local_dns"
      strategy: "roundrobin"
      # 重要：这里的 URL 指向你的本地传统 DNS 服务器的 DoH 封装
      # 如果你的本地 DNS 不支持 DoH，你需要一个转换器，
      # 或者使用支持直接转发到 UDP/TCP 的 DNS 代理。
      # 注意：当前版本的 Load Ants 只支持 DoH 上游。
      # 这里的示例是一个假设性的 DoH 转换器地址。
      servers:
          - url: "http://192.168.1.53:8053/dns-query" # 假设你有一个 DoH 转换器

# ----------------------------------
# 路由规则
# ----------------------------------
static_rules:
    # --- 本地域名解析规则 ---
    # 将所有以 .lan 结尾的域名查询，都转发到本地 DNS 组
    - match: "regex"
      patterns: ["(^.*\\.lan$)|(^lan$)"]
      action: "forward"
      target: "local_dns"

    # --- 默认规则 ---
    # 其他所有查询都转发到公共 DNS
    - match: "wildcard"
      patterns: ["*"]
      action: "forward"
      target: "public_dns"
```

### 步骤二：配置你的网络

将你的设备或路由器的 DNS 设置指向运行 Load Ants 的主机。

**工作原理**:

1.  当一个对 `nas.lan` 的查询到达 Load Ants 时，它会匹配第一条 `static_rules` 规则（正则表达式匹配）。
2.  该规则的 `action` 是 `forward`，`target` 是 `local_dns` 上游组。
3.  Load Ants 随即将该查询通过 DoH 转发给你配置的本地 DNS 服务 (`http://192.168.1.53:8053/dns-query`)。
4.  你的本地 DNS 服务解析 `nas.lan` 到 `192.168.1.10` 并返回结果。
5.  当一个对 `www.google.com` 的查询到达时，它无法匹配第一条规则，于是匹配了第二条 `wildcard` 规则，被转发到 `public_dns` 组，并由谷歌的 DoH 服务器解析。

这个方法虽然比想象中复杂，但它正确地利用了 Load Ants 的核心能力，并实现了稳定、可扩展的本地网络 DNS 管理。

> **注意：关于 DoH 转换器**
>
> 当前版本的 Load Ants **只支持 DoH 上游**。如果你的本地 DNS 服务器（如 dnsmasq）只提供传统的 UDP/53 端口，你将需要一个额外的软件（如 `dns-over-https/doh-server`）来将传统 DNS 查询封装成 DoH。上述配置示例假设了这样一个转换器正在运行。在未来的版本中，Load Ants 可能会支持直接转发到 UDP/TCP 上游，从而简化此配置。

---

### 下一步

-   [➡️ 学习广告拦截配方](./ad-blocking.md)
-   [➡️ 回顾路由规则配置](../configuration/routing-rules.md)
-   [➡️ 返回实例总览](./index.md)
