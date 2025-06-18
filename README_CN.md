[English](./README.md) | 中文

<div align="center">
    <h1>LOAD ANTS</h1>
    <h4>🐜🐜🐜 轻量级 DNS 转发器，实现 UDP/TCP 查询到 DoH 的无缝转换</h4></br>
    <img src="./images/logo.png" alt="logo" width="600">
</div>

<p align="center">
  <a href="#项目介绍">简介</a> |
  <a href="#快速开始">快速开始</a> |
  <a href="#核心功能">核心功能</a> |
  <a href="#配置详解">配置详解</a> |
  <a href="#安装与进阶使用">安装与进阶使用</a> |
  <a href="#深入了解">深入了解</a>
</p>

## 项目介绍

**Load Ants** 是一款高性能、多功能的 DNS 代理服务，能够将传统的 UDP/TCP DNS 查询转换为 DNS-over-HTTPS (DoH)。它作为使用标准 DNS 协议的客户端与现代安全 DoH 提供商之间的桥梁，提供增强的隐私保护、安全性和灵活的路由功能。

### 为什么选择 DNS-over-HTTPS？

传统 DNS 查询采用明文传输，这使你的浏览历史容易遭受监控、劫持或篡改。DoH 通过以下方式解决这些问题：

-   **DNS 流量加密** - 防止网络中间人窥探
-   **隐私增强** - 隐藏 DNS 查询内容，避免被运营商和其他网络观察者捕获
-   **安全性提升** - 有效降低 DNS 投毒和欺骗攻击的风险
-   **突破网络限制** - 帮助规避基于 DNS 的网络封锁技术

## 快速开始

本节将指导你快速部署并运行 Load Ants。我们推荐首先尝试直接运行预编译的应用程序，如果方便，也可以使用 Docker。

### 环境要求

-   **通用**:
    -   一个文本编辑器，用于创建和修改配置文件 (`config.yaml`)。
    -   管理员/root 权限 (如果 Load Ants 需要绑定标准 DNS 端口 53)。
-   **直接运行预编译程序**:
    -   从项目 [发布页面](https://github.com/shengyanli1982/load-ants/releases) 下载的对应你操作系统的二进制文件。
-   **使用 Docker**:
    -   Docker 已安装并正在运行。
-   **(可选) 从源码构建 (进阶)**:
    -   Rust 工具链 (Rust 1.84.1、 GCC 14.2.0)。

### 方法一：直接运行应用程序

这是体验 Load Ants 最快捷的方式，特别是如果你希望在本机直接运行。

1.  **下载预编译版本**:
    访问项目的 [发布页面](https://github.com/shengyanli1982/load-ants/releases)，找到并下载适用于你操作系统的最新 `load-ants` 二进制文件。

2.  **准备配置文件**:
    下载或复制项目中的示例配置文件 `config.default.yaml` (通常与源代码一同提供，或附在发布版本中)。将其与下载的 `load-ants` 二进制文件放在同一个目录下，并重命名为 `config.yaml`。

    ```bash
    # 假设 config.default.yaml 已获取并放置在当前目录
    cp config.default.yaml ./config.yaml
    ```

    然后，使用你喜欢的文本编辑器打开 `config.yaml` 并进行修改。至少，你需要配置一个上游 DoH 服务器组 (`upstream_groups`)。可以参考文档后续的 [配置详解](#配置详解) 部分。

3.  **赋予执行权限 (Linux/macOS)**:
    假如你使用的是 Linux （x86_64）系统，你可能需要给下载的二进制文件添加执行权限：

    > ![NOTE]
    > 假如你使用的是其他系统，请根据实际情况调整。

    ```bash
    chmod +x ./loadants-linux-amd64
    ```

4.  **运行 Load Ants**:
    打开终端，导航到包含 `loadants` 和 `config.yaml` 文件的目录，然后执行：

    ```bash
    ./loadants-linux-amd64 -c ./config.yaml
    ```

    如果 Load Ants 配置为监听标准 DNS 端口 (如 53)，你可能需要使用管理员权限来启动它 (例如，在 Linux/macOS 上使用 `sudo ./loadants -c ./config.yaml`，在 Windows 上以管理员身份运行命令提示符或 PowerShell)。
    程序启动后，它会根据配置文件开始处理 DNS 请求。日志信息会直接输出到终端。

5.  **基本测试**:
    程序运行后，你可以使用 `dig` (Linux/macOS) 或 `nslookup` (Windows) 等工具测试 DNS 解析：

    ```bash
    # 假设 Load Ants 监听在 127.0.0.1:53
    dig @127.0.0.1 example.com
    ```

    如果一切配置正确，你应该能收到来自上游 DoH 服务器的 DNS 响应。

6.  **停止程序**:
    在运行 Load Ants 的终端中，按下 `Ctrl+C` 来停止程序。

### 方法二：使用 Docker 部署 (推荐)

Docker 提供了一种隔离环境来运行 Load Ants，无需在你的系统上直接安装任何依赖 (除了 Docker 本身)。

1.  **创建配置目录**:
    在你的宿主机上创建一个目录来存放配置文件。

    ```bash
    mkdir -p ./load-ants-config
    ```

2.  **准备配置文件**:
    复制项目中的默认配置文件 (`config.default.yaml`) 到你刚创建的目录，并根据你的需求进行编辑，将其命名为 `config.yaml`。

    ```bash
    # 假设你已克隆项目或下载了 config.default.yaml
    cp config.default.yaml ./load-ants-config/config.yaml
    # 使用你喜欢的编辑器修改 ./load-ants-config/config.yaml
    ```

    你至少需要配置上游 DoH 服务器 (`upstream_groups`)。

3.  **运行 Load Ants 容器**:

    ```bash
    docker run -d \
      --name load-ants \
      -p 53:53/udp \
      -p 53:53/tcp \
      -p 8080:8080 \
      -v $(pwd)/load-ants-config:/etc/load-ants \
      ghcr.io/shengyanli1982/load-ants-x64:latest -c /etc/load-ants/config.yaml
    ```

    请将 `ghcr.io/shengyanli1982/load-ants-x64:latest` 替换为你自己构建的镜像名称或官方提供的最新镜像。
    命令解释:

    -   `-d`: 后台运行容器。
    -   `--name load-ants`: 为容器命名。
    -   `-p 53:53/udp -p 53:53/tcp`: 将宿主机的 DNS 端口映射到容器。
    -   `-p 8080:8080`: 映射管理端口 (健康检查、指标)。
    -   `-v $(pwd)/load-ants-config:/etc/load-ants`: 将宿主机的配置目录挂载到容器内。请确保 `$(pwd)/load-ants-config` 解析为绝对路径，或者直接提供绝对路径。
    -   `ghcr.io/shengyanli1982/load-ants-x64:latest`: 使用的 Docker 镜像。
    -   `-c /etc/load-ants/config.yaml`: 指定容器内配置文件的路径。

4.  **基本测试**:
    容器启动后，你可以通过以下方式测试服务：

    -   **测试 DNS 解析**:
        使用 `dig` 或其他 DNS 查询工具向 `127.0.0.1` (或你的服务器 IP) 发送查询请求。
        ```bash
        dig @127.0.0.1 example.com
        ```
        如果配置正确，你应该能收到来自上游 DoH 服务器的响应。
    -   **查看日志**:
        检查 Load Ants 的运行日志，了解其工作状态或排查问题。
        ```bash
        docker logs load-ants
        ```

5.  **停止和移除容器** (如果需要):
    ```bash
    docker stop load-ants
    docker rm load-ants
    ```

## 核心功能

-   🔄 **协议转换**
    -   无缝将 UDP/53 和 TCP/53 DNS 请求转换为 DoH (RFC 8484)
    -   全面支持 GET 和 POST HTTP 方法
    -   处理多种内容格式，包括 `application/dns-message` 和 `application/dns-json`
-   🧠 **智能路由**
    -   **灵活匹配** - 根据域名模式精准路由 DNS 查询：
        -   精确域名匹配
        -   通配符域名匹配（如 `*.example.com`）
        -   正则表达式域名匹配
    -   **自定义操作** - 为每次匹配定义精确处理方式：
        -   转发至特定上游 DoH 组
        -   拦截查询（返回 NXDOMAIN）
    -   **远程规则列表** - 支持从 URL 加载和合并外部规则列表 (例如 V2Ray 格式的 `reject-list.txt`, `proxy-list.txt`)
-   🌐 **灵活的上游管理**
    -   **分组机制** - 将 DoH 服务器组织成独立配置的逻辑组
    -   **负载均衡** - 为每个组配置高效均衡策略：
        -   轮询 (RR) - 在服务器之间均衡分配请求
        -   加权轮询 (WRR) - 根据权重优先调度服务器
        -   随机分配 - 非确定性选择，增强隐私保护
    -   **认证支持** - 与需要认证的私有 DoH 提供商安全通信：
        -   HTTP 基本认证
        -   Bearer 令牌认证
    -   **资源优化** - 所有上游组共享 HTTP 客户端连接池，提升资源利用率
-   ⚡ **性能优化**
    -   **智能缓存** - 内置 DNS 缓存机制，显著降低延迟和上游负载
        -   **正向缓存** - 存储成功的 DNS 响应，加速解析过程
        -   **负向缓存** - 缓存错误响应（NXDOMAIN、ServFail 等），避免对不存在域名的重复查询
        -   **可调整 TTL** - 为正向和负向缓存条目设置差异化的生存时间
    -   **连接池复用** - 高效复用 HTTP 连接，提升性能
    -   **TTL 优化** - 灵活配置缓存响应的最小和最大 TTL 值
-   🔁 **高可靠性**
    -   **智能重试** - 自动重试失败的 DoH 请求，支持可配置的尝试次数和延迟
    -   **超时控制** - 精确调整连接和请求超时参数
-   ⚙️ **管理能力**
    -   **YAML 配置** - 简洁、易读的配置方式
    -   **配置校验** - 启动时或测试模式下进行严格配置验证
    -   **健康检查** - 为运维团队提供完整的监控集成接口
    -   **Prometheus 指标** - 通过 `/metrics` 端点提供全面的监控指标

## 配置详解

Load Ants 使用 YAML 格式的配置文件。以下是完整的配置选项参考。建议从 `config.default.yaml` 开始修改。

### 服务器配置 (server)

| 参数         | 类型   | 默认值         | 描述                                        | 有效范围            |
| ------------ | ------ | -------------- | ------------------------------------------- | ------------------- |
| listen_udp   | 字符串 | "127.0.0.1:53" | UDP DNS 监听地址和端口（必选）              | 有效的 IP:端口 格式 |
| listen_tcp   | 字符串 | "127.0.0.1:53" | TCP DNS 监听地址和端口（必选）              | 有效的 IP:端口 格式 |
| listen_http  | 字符串 | "127.0.0.1:80" | HTTP DoH 服务器监听地址和端口（必选）       | 有效的 IP:端口 格式 |
| tcp_timeout  | 整数   | 10             | TCP 连接空闲超时（秒）（可选，默认值: 10）  | 1-3600              |
| http_timeout | 整数   | 30             | HTTP 连接空闲超时（秒）（可选，默认值: 30） | 1-3600              |

### 健康检查配置 (health)

此部分配置用于暴露健康检查和监控指标的 HTTP 服务。

| 参数   | 类型   | 默认值           | 描述                                                       | 有效范围            |
| ------ | ------ | ---------------- | ---------------------------------------------------------- | ------------------- |
| listen | 字符串 | "127.0.0.1:8080" | 健康检查服务监听地址和端口（如果提供 health 部分，则必选） | 有效的 IP:端口 格式 |

### 缓存配置 (cache)

缓存配置允许精细调整 DNS 响应的缓存行为。

| 参数         | 类型   | 默认值 | 描述                                                                              | 有效范围   |
| ------------ | ------ | ------ | --------------------------------------------------------------------------------- | ---------- |
| enabled      | 布尔值 | true   | 是否启用缓存（如果提供 cache 部分，则必选）                                       | true/false |
| max_size     | 整数   | 10000  | 最大缓存条目数（如果提供 cache 部分，则必选）                                     | 10-1000000 |
| min_ttl      | 整数   | 60     | 最小 TTL（秒），会覆盖原始响应中更小的 TTL 值（如果提供 cache 部分，则必选）      | 1-86400    |
| max_ttl      | 整数   | 3600   | 所有缓存条目的最大生存时间上限（秒）（如果提供 cache 部分，则必选）               | 1-86400    |
| negative_ttl | 整数   | 300    | 负向缓存 TTL（秒），用于缓存错误、不存在域名等响应（如果提供 cache 部分，则必选） | 1-86400    |

**负向缓存说明**:
负向缓存是一种重要的性能优化技术，它将 DNS 错误响应（如 NXDOMAIN 或 ServFail）缓存指定时间。这能有效防止对不存在或暂时无法解析的域名重复查询上游服务器，从而减少延迟并降低上游服务器负载。

### HTTP 客户端配置 (http_client)

此配置应用于所有发向上游 DoH 服务器的 HTTP 请求。

| 参数            | 类型   | 默认值 | 描述                                                | 有效范围   |
| --------------- | ------ | ------ | --------------------------------------------------- | ---------- |
| connect_timeout | 整数   | 5      | 连接超时（秒）（如果提供 http_client 部分，则必选） | 1-120      |
| request_timeout | 整数   | 10     | 请求超时（秒）（如果提供 http_client 部分，则必选） | 1-1200     |
| idle_timeout    | 整数   | 60     | 空闲连接超时（秒）（可选）                          | 5-1800     |
| keepalive       | 整数   | 60     | TCP Keepalive（秒）（可选）                         | 5-600      |
| agent           | 字符串 | -      | HTTP 用户代理（可选）                               | 非空字符串 |

### 上游 DoH 服务器组配置 (upstream_groups)

你可以定义一个或多个上游 DoH 服务器组，每个组可以包含多个服务器，并具有独立的负载均衡策略、重试机制和代理设置。

| 参数     | 类型   | 默认值 | 描述                                     | 有效范围                           |
| -------- | ------ | ------ | ---------------------------------------- | ---------------------------------- |
| name     | 字符串 | -      | 组名称 (必选, 需唯一)                    | 非空字符串                         |
| strategy | 字符串 | -      | 负载均衡策略 (必选)                      | "roundrobin", "weighted", "random" |
| servers  | 数组   | -      | 此组中的 DoH 服务器列表 (必选, 至少一个) | -                                  |
| retry    | 对象   | -      | 针对此组的请求重试配置（可选）           | 参照下方重试配置                   |
| proxy    | 字符串 | -      | 访问此组服务器时使用的代理 (可选)        | 有效的 HTTP/SOCKS5 代理 URL        |

#### 服务器配置 (servers)

`upstream_groups` 中 `servers` 数组的每个元素代表一个 DoH 服务器。

| 参数         | 类型   | 默认值    | 描述                                                                                       | 有效范围                     |
| ------------ | ------ | --------- | ------------------------------------------------------------------------------------------ | ---------------------------- |
| url          | 字符串 | -         | DoH 服务器 URL (必选)                                                                      | 有效的 HTTP(S) URL，包含路径 |
| weight       | 整数   | 1         | 权重 (仅当组策略为 `weighted` 时有效) (可选，默认值: 1)                                    | 1-65535                      |
| method       | 字符串 | "post"    | DoH 请求方法 (GET 或 POST) (可选，默认值: post)                                            | "get", "post"                |
| content_type | 字符串 | "message" | DoH 内容类型 (`application/dns-message` 或 `application/dns-json`) (可选，默认值: message) | "message", "json"            |
| auth         | 对象   | -         | 访问此服务器的认证配置（可选）                                                             | 参照下方认证配置             |

**DoH 内容类型的技术因素：**

-   `message` (`application/dns-message`)：实现 RFC 8484 标准协议，同时支持 GET 和 POST 两种 HTTP 方法。该格式直接封装二进制 DNS 消息，是跨 DoH 提供商实现最佳兼容性和性能的推荐选项。

-   `json` (`application/dns-json`)：实现 Google 的 DNS 查询 JSON API 规范，该规范**仅支持 GET 方法**。此格式主要用于兼容需要 JSON 格式响应的特定客户端实现。

配置 `content_type: "json"` 时，你**必须**同时指定 `method: "get"`。系统的配置验证器强制执行此协议要求，并将拒绝任何尝试将 `content_type: "json"` 与 `method: "post"` 组合的配置，因为这种组合违反了 Google Public DNS 规范，会导致查询失败。

#### 认证配置 (auth)

用于 `upstream_groups.servers.auth`。

| 参数     | 类型   | 默认值 | 描述                                   | 有效范围          |
| -------- | ------ | ------ | -------------------------------------- | ----------------- |
| type     | 字符串 | -      | 认证类型 (如果提供 auth 部分，则必选)  | "basic", "bearer" |
| username | 字符串 | -      | 用户名（如果 type 为 `basic`，则必选） | 非空字符串        |
| password | 字符串 | -      | 密码（如果 type 为 `basic`，则必选）   | 非空字符串        |
| token    | 字符串 | -      | 令牌（如果 type 为 `bearer`，则必选）  | 非空字符串        |

#### 重试配置 (retry)

用于 `upstream_groups.retry`。

| 参数     | 类型 | 默认值 | 描述                             | 有效范围 |
| -------- | ---- | ------ | -------------------------------- | -------- |
| attempts | 整数 | 3      | 重试次数 (可选，默认值: 3)       | 1-100    |
| delay    | 整数 | 1      | 初始延迟（秒） (可选，默认值: 1) | 1-120    |

### 路由规则配置 (static_rules)

定义本地静态路由规则。Load Ants 采用基于优先级的匹配系统进行 DNS 路由决策：

1.  **精确匹配** (`exact`) - 完全匹配完整域名（如 `example.com`）。最高优先级。
2.  **特定通配符匹配** (`wildcard`) - 使用通配符匹配特定域名模式（如 `*.example.com`）。
3.  **正则表达式匹配** (`regex`) - 使用正则表达式进行复杂匹配（如 `^(mail|audio)\\.google\\.com$`）。
4.  **全局通配符匹配** (`wildcard` 模式为 `*`) - 匹配任何域名。最低优先级。

通常，全局通配符（`*`）应作为最后一条规则，作为其他规则都不匹配时的默认选项。

| 参数     | 类型   | 默认值 | 描述                                         | 有效范围                     |
| -------- | ------ | ------ | -------------------------------------------- | ---------------------------- |
| match    | 字符串 | -      | 匹配类型 (必选)                              | "exact", "wildcard", "regex" |
| patterns | 数组   | -      | 匹配模式列表 (必选, 至少一个模式)            | 非空字符串数组               |
| action   | 字符串 | -      | 路由动作 (必选)                              | "forward", "block"           |
| target   | 字符串 | -      | 目标上游组 (当 `action` 为 `forward` 时必选) | 已定义的上游组名称           |

### 远程规则配置 (remote_rules)

`remote_rules` 允许系统从外部 URL 获取域名规则列表（如阻止列表、代理列表等），并与本地静态规则合并。这些规则将根据其 `action`（block 或 forward）和从远程文件解析出的匹配类型（exact, wildcard, regex）整合到路由引擎中，并遵循与静态规则相同的优先级逻辑。

| 参数     | 类型   | 默认值   | 描述                                                                        | 有效范围                             |
| -------- | ------ | -------- | --------------------------------------------------------------------------- | ------------------------------------ |
| type     | 字符串 | "url"    | 规则类型，目前仅支持 "url" (必选)                                           | "url"                                |
| url      | 字符串 | -        | 远程规则文件的 URL (必选)                                                   | 有效的 HTTP(S) URL                   |
| format   | 字符串 | "v2ray"  | 规则文件格式 (必选)                                                         | "v2ray" (未来可能支持 "clash" 等)    |
| action   | 字符串 | -        | 应用于此规则列表中所有域名的动作 (必选)                                     | "block", "forward"                   |
| target   | 字符串 | -        | 目标上游组 (当 `action` 为 `forward` 时必选)                                | 已定义的上游组名称                   |
| retry    | 对象   | -        | 获取规则的重试策略 (可选)                                                   | 参照下方 `remote_rules` 内的重试配置 |
| proxy    | 字符串 | -        | 获取规则时使用的 HTTP/SOCKS5 代理 (可选)                                    | 有效的代理 URL                       |
| auth     | 对象   | -        | 访问远程规则 URL 的认证配置 (可选)                                          | 参照下方 `remote_rules` 内的认证配置 |
| max_size | 整数   | 10485760 | 远程规则文件的最大大小 (字节), 例如 10485760 表示 10MB (可选，默认值: 10MB) | 1 - N (例如 10485760 for 10MB)       |

#### 重试配置 (retry) - remote_rules 内

用于 `remote_rules.retry`。

| 参数     | 类型 | 默认值 | 描述                             | 有效范围 |
| -------- | ---- | ------ | -------------------------------- | -------- |
| attempts | 整数 | 3      | 重试次数 (可选，默认值: 3)       | 1-100    |
| delay    | 整数 | 1      | 初始延迟（秒） (可选，默认值: 1) | 1-120    |

#### 认证配置 (auth) - remote_rules 内

用于 `remote_rules.auth`，结构与 `upstream_groups.servers.auth` 相同。

| 参数     | 类型   | 默认值 | 描述                                   | 有效范围          |
| -------- | ------ | ------ | -------------------------------------- | ----------------- |
| type     | 字符串 | -      | 认证类型 (如果提供 auth 部分，则必选)  | "basic", "bearer" |
| username | 字符串 | -      | 用户名（如果 type 为 `basic`，则必选） | 非空字符串        |
| password | 字符串 | -      | 密码（如果 type 为 `basic`，则必选）   | 非空字符串        |
| token    | 字符串 | -      | 令牌（如果 type 为 `bearer`，则必选）  | 非空字符串        |

**`remote_rules` 示例:**

```yaml
remote_rules:
    # 从URL获取阻止列表，并使用Bearer认证和代理
    - type: "url"
      url: "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/reject-list.txt"
      format: "v2ray"
      action: "block"
      retry:
          attempts: 3
          delay: 1
      proxy: "http://user:pass@proxyserver:port"
      auth:
          type: "bearer"
          token: "your_secure_token"
      max_size: 1048576 # 1MB

    # 从URL获取转发列表，指定目标上游组
    - type: "url"
      url: "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/proxy-list.txt"
      format: "v2ray"
      action: "forward"
      target: "google_public" # 转发到名为 "google_public" 的上游组
```

### 配置示例

这是一个包含大部分常用配置的完整示例：

```yaml
# Load Ants 配置示例

# 服务器监听设置
server:
    listen_udp: "0.0.0.0:53" # UDP 监听地址和端口
    listen_tcp: "0.0.0.0:53" # TCP 监听地址和端口
    listen_http: "0.0.0.0:80" # HTTP DoH 服务器监听地址和端口
    tcp_timeout: 10 # TCP连接空闲超时(秒)
    http_timeout: 30 # HTTP连接空闲超时(秒)

# 健康检查与管理服务器设置
admin:
    listen: "0.0.0.0:8080" # 健康检查服务器监听地址和端口

# 缓存设置
cache:
    enabled: true
    max_size: 10000 # 最大缓存条目数
    min_ttl: 60 # 缓存条目最小 TTL（秒）
    max_ttl: 3600 # 缓存条目最大 TTL（秒）
    negative_ttl: 300 # 负面缓存 TTL（秒），用于缓存错误响应

# HTTP 客户端设置 (全局)
http_client:
    connect_timeout: 5 # 连接超时（秒）
    request_timeout: 10 # 请求超时（秒）
    idle_timeout: 60 # 空闲连接超时（秒） (可选)
    keepalive: 60 # TCP Keepalive（秒） (可选)
    agent: "LoadAnts/1.0" # 自定义 User-Agent (可选)

# 上游 DoH 服务器组
upstream_groups:
    - name: "google_public"
      strategy: "roundrobin" # 可选: roundrobin, weighted, random
      servers:
          - url: "https://dns.google/dns-query"
            method: "post" # 可选: get, post (默认 post)
            content_type: "message" # 可选: message, json (默认 message)
          - url: "https://8.8.4.4/dns-query"
            method: "get"
      retry: # 当前组的重试策略 (可选)
          attempts: 3
          delay: 1 # 秒
      proxy: "http://user:pass@proxyserver:port" # 当前组的代理 (可选)

    - name: "cloudflare_secure"
      strategy: "random"
      servers:
          - url: "https://cloudflare-dns.com/dns-query"
          - url: "https://1.0.0.1/dns-query"
            content_type: "json"

    - name: "nextdns_authenticated"
      strategy: "weighted"
      servers:
          - url: "https://dns.nextdns.io/YOUR_CONFIG_ID_1"
            weight: 70
            auth: # 特定服务器的认证 (可选)
                type: "bearer" # basic 或 bearer
                token: "YOUR_API_KEY_OR_TOKEN_1"
          - url: "https://dns.nextdns.io/YOUR_CONFIG_ID_2" # 注意：通常 NextDNS 使用相同的配置ID
            weight: 30
            auth:
                type: "basic"
                username: "your_username"
                password: "your_password"
      # retry: # 这个组可以有自己的重试策略 (可选)
      # proxy: # 这个组也可以有自己的代理 (可选)

# 路由规则（静态配置）
static_rules:
    # 阻止特定域名
    - match: "exact"
      patterns: ["ads.example.com", "tracker.example.org"]
      action: "block"

    # 将内部域名路由到特定上游组
    - match: "wildcard"
      patterns: ["*.corp.internal", "*.mycompany.local"]
      action: "forward"
      target: "cloudflare_secure" # 引用上面定义的上游组名称

    # 使用正则表达式匹配并转发
    - match: "regex"
      patterns: ["^(.*\.)?google\.com$", "^(.*\.)?gstatic\.com$"] # 匹配 google.com 及其子域名
      action: "forward"
      target: "google_public"

    # 默认规则：将所有其他流量转发到 google_public (确保这是最后一条规则)
    - match: "wildcard"
      patterns: ["*"] # 匹配所有域名
      action: "forward"
      target: "google_public"

# 远程规则配置 (从 URL 加载规则)
remote_rules:
  # 示例：从URL加载V2Ray格式的阻止列表
  - type: "url"
    url: "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/reject-list.txt"
    format: "v2ray" # 目前支持 v2ray
    action: "block" # 规则列表中的域名将被阻止
    retry: # 获取此规则列表的重试策略 (可选)
      attempts: 3
      delay: 2
    proxy: "socks5://localhost:1080" # 获取此规则列表时使用的代理 (可选)
    auth: # 获取此规则列表的认证 (可选)
      type: "bearer"
      token: "some_token"
    max_size: 2097152 # 此规则文件的最大大小 (例如 2MB)

  # 示例：从URL加载V2Ray格式的代理(转发)列表
  - type: "url"
    url: "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/direct-list.txt" # 通常 direct-list 用于直连，这里假设用作转发
    format: "v2ray"
    action: "forward"
    target: "cloudflare_secure" # 规则列表中的域名将被转发到 cloudflare_secure 组
```

## 安装与进阶使用

### 命令行参数

```text
load-ants [OPTIONS]

选项:
    -c, --config <PATH>    指定配置文件路径
                           (默认查找顺序: ./config.yaml, /etc/load-ants/config.yaml)
    -t, --test             测试配置文件有效性并退出
    -h, --help             显示帮助信息
    -V, --version          显示版本信息
```

### 从源代码构建

如果你希望自行构建或修改代码：

1.  **环境准备**:

    -   安装 [Rust 工具链](https://www.rust-lang.org/tools/install) (Rust 1.84.1, GCC 14.2.0)。
    -   确保你的系统已安装 `git`。

2.  **克隆代码仓库**:

    ```bash
    git clone https://github.com/shengyanli1982/load-ants.git
    cd load-ants
    ```

    (请将 `shengyanli1982/load-ants.git` 替换为实际的仓库地址)

3.  **构建应用程序**:

    ```bash
    cargo build --release
    ```

    编译后的二进制文件位于 `target/release/load-ants`。

4.  **运行**:
    你需要一个配置文件 (例如，从 `config.default.yaml` 复制并修改)。
    ```bash
    # 假设配置文件在 ./config.yaml
    sudo ./target/release/load-ants -c ./config.yaml
    ```
    如果需要监听标准 DNS 端口 (53)，通常需要 `sudo` 权限。

编译后的二进制文件也可以直接从项目的[发布页面](https://github.com/shengyanli1982/load-ants/releases)下载（如果提供）。

### 作为系统服务使用 (Linux systemd)

如果你希望将 Load Ants 作为后台服务运行在 Linux 系统上：

1.  **准备二进制文件和配置文件**:

    -   将编译好的 `load-ants` 二进制文件放置到一个合适的系统路径，例如 `/usr/local/bin/load-ants`。
    -   将你的配置文件放置到例如 `/etc/load-ants/config.yaml`。
        ```bash
        sudo mkdir -p /etc/load-ants
        sudo cp /path/to/your/config.yaml /etc/load-ants/config.yaml
        sudo cp ./target/release/load-ants /usr/local/bin/
        sudo chmod +x /usr/local/bin/load-ants
        ```

2.  **创建服务文件**:
    创建 `/etc/systemd/system/load-ants.service` 文件，内容如下：

    ```ini
    [Unit]
    Description=Load Ants DNS to DoH Proxy Service
    After=network.target network-online.target
    Requires=network-online.target

    [Service]
    Type=simple
    ExecStart=/usr/local/bin/load-ants -c /etc/load-ants/config.yaml
    Restart=on-failure
    RestartSec=5s
    User=root # 或其他有权限绑定53端口的用户，如果是非root用户，可能需要设置CAP_NET_BIND_SERVICE
    Group=root # 或对应的用户组
    # AmbientCapabilities=CAP_NET_BIND_SERVICE # 如果以非 root 用户运行且需绑定特权端口

    # 安全性增强 (可选)
    ProtectSystem=full
    ProtectHome=true
    PrivateTmp=true
    NoNewPrivileges=true

    [Install]
    WantedBy=multi-user.target
    ```

3.  **重载 systemd 配置并启动服务**:

    ```bash
    sudo systemctl daemon-reload
    sudo systemctl enable load-ants.service
    sudo systemctl start load-ants.service
    ```

4.  **检查服务状态**:
    ```bash
    sudo systemctl status load-ants.service
    journalctl -u load-ants.service -f # 查看实时日志
    ```

### Kubernetes 部署方案

对于生产环境，Kubernetes 提供了更好的扩展性、高可用性和管理便捷性。

1.  **创建 Docker 镜像 (如果尚未发布)**:
    如果你的项目包含 `Dockerfile`，你需要先构建并推送 Docker 镜像到镜像仓库 (如 Docker Hub, GCR, ECR)。

    ```bash
    # 假设 Dockerfile 在项目根目录
    docker build -t yourusername/load-ants:latest .
    docker push yourusername/load-ants:latest
    ```

    (请将 `yourusername/load-ants:latest` 替换为你的实际镜像名称和标签)

2.  **创建 ConfigMap**:
    将你的 `config.yaml` 内容存储在 Kubernetes ConfigMap 中。

    ```yaml
    # load-ants-configmap.yaml
    apiVersion: v1
    kind: ConfigMap
    metadata:
        name: load-ants-config
        namespace: dns # 建议为DNS相关服务使用独立的命名空间
    data:
        config.yaml: |
            # 在这里粘贴你的完整 config.yaml 内容
            server:
              listen_udp: "0.0.0.0:53"
              listen_tcp: "0.0.0.0:53"
              listen_http: "0.0.0.0:80"
            admin:
              listen: "0.0.0.0:8080"
            cache:
              enabled: true
              max_size: 10000
              # ... 其他配置 ...
            upstream_groups:
              - name: "google_public"
                strategy: "roundrobin"
                servers:
                  - url: "https://dns.google/dns-query"
                # ... 更多上游配置 ...
            # ... 更多规则配置 ...
    ```

3.  **创建 Deployment**:
    定义 Deployment 来管理 Load Ants Pod。

    ```yaml
    # load-ants-deployment.yaml
    apiVersion: apps/v1
    kind: Deployment
    metadata:
        name: load-ants
        namespace: dns
        labels:
            app: load-ants
    spec:
        replicas: 2 # 根据需求调整副本数
        selector:
            matchLabels:
                app: load-ants
        template:
            metadata:
                labels:
                    app: load-ants
            spec:
                containers:
                    - name: load-ants
                      image: ghcr.io/shengyanli1982/load-ants-x64:latest # 使用官方或你自己构建的镜像
                      args: ["-c", "/etc/load-ants/config.yaml"]
                      ports:
                          - containerPort: 53
                            name: dns-udp
                            protocol: UDP
                          - containerPort: 53
                            name: dns-tcp
                            protocol: TCP
                          - containerPort: 8080
                            name: http-admin # 用于健康检查和指标
                      volumeMounts:
                          - name: config-volume
                            mountPath: /etc/load-ants # 挂载配置文件目录
                      resources: # 根据实际情况调整资源请求和限制
                          limits:
                              memory: "256Mi"
                              cpu: "500m"
                          requests:
                              memory: "128Mi"
                              cpu: "100m"
                      livenessProbe: # 健康检查
                          httpGet:
                              path: /health
                              port: http-admin
                          initialDelaySeconds: 15
                          periodSeconds: 20
                      readinessProbe: # 就绪探针
                          httpGet:
                              path: /health
                              port: http-admin
                          initialDelaySeconds: 5
                          periodSeconds: 10
                volumes:
                    - name: config-volume
                      configMap:
                          name: load-ants-config # 引用上面创建的 ConfigMap
    ```

4.  **创建 Service**:
    暴露 Load Ants 服务，使其可以在集群内部或外部访问。

    ```yaml
    # load-ants-service.yaml
    apiVersion: v1
    kind: Service
    metadata:
        name: load-ants-svc
        namespace: dns
    spec:
        selector:
            app: load-ants # 匹配 Deployment 中的 Pod 标签
        ports:
            - name: dns-udp
              port: 53
              protocol: UDP
              targetPort: dns-udp
            - name: dns-tcp
              port: 53
              protocol: TCP
              targetPort: dns-tcp
            - name: http-admin
              port: 8080
              protocol: TCP
              targetPort: http-admin
        # type: ClusterIP # 默认，仅集群内部访问。集群内部其他Pod可通过 load-ants-svc.dns:53 访问。
        type: LoadBalancer # 如果需要从外部访问，并由云提供商支持 (会分配公网IP)。
        # type: NodePort # 如果需要在每个节点的特定端口上暴露服务。
    ```

5.  **应用配置到集群**:

    ```bash
    kubectl create namespace dns # 如果尚未创建
    kubectl apply -f load-ants-configmap.yaml
    kubectl apply -f load-ants-deployment.yaml
    kubectl apply -f load-ants-service.yaml
    ```

6.  **检查部署状态**:
    ```bash
    kubectl -n dns get pods -l app=load-ants
    kubectl -n dns get svc load-ants-svc
    kubectl -n dns logs -l app=load-ants -f # 查看 Pod 实时日志
    ```

## 深入了解

### 架构设计

Load Ants 采用模块化架构设计，包含以下核心组件：

-   **服务器模块 (`server`)**: 监听 UDP/53 和 TCP/53 端口，接收传统的 DNS 查询。
-   **DNS 解析器 (`parser` & `composer`)**: 解析传入的 DNS 请求，构建 DNS 响应。
-   **处理器模块 (`processor`)**: 协调查询处理流程，包括缓存检查、路由决策和上游转发。
-   **缓存模块 (`cache`)**: 实现高效的 DNS 缓存（正向和负向），减少延迟，降低上游负载。
-   **路由模块 (`router`)**: 根据配置的规则（静态和远程）匹配域名，决定查询的动作（转发或阻止）和目标。
-   **上游管理模块 (`upstream`)**: 管理 DoH 上游服务器组，处理与 DoH 服务器的 HTTP(S) 通信，实现负载均衡、认证和重试逻辑。
-   **HTTP 客户端 (`http_client`)**: 全局共享的 HTTP 客户端，用于与上游 DoH 服务器以及远程规则 URL 通信。
-   **远程规则加载器 (`remote_rule`)**: 负责从 URL 获取、解析和转换远程规则列表，支持重试、代理和认证。
-   **管理与监控模块 (`admin`/`health`/`metrics`)**: 提供 HTTP 端点用于健康检查 (`/health`)、Prometheus 指标 (`/metrics`) 和缓存刷新 (`/api/cache/refresh`)。

![architecture](./images/architecture.png)
_图：Load Ants 架构示意图_

### Prometheus 监控指标

Load Ants 提供完整的 Prometheus 监控指标，用于实时监控服务性能、健康状态和运行情况。这些指标通过 `/metrics` 端点暴露 (默认监听在 `0.0.0.0:8080/metrics`，可通过 `health.listen` 配置修改)，可被 Prometheus 或其他兼容的监控系统采集。

![metrics](./images/metrics.png)
_图：Load Ants Prometheus 指标示例 (Grafana 仪表盘)_

#### 主要指标分类：

-   **DNS 请求指标**:
    -   `loadants_dns_requests_total`: 处理的 DNS 请求总数 (标签: `protocol` (UDP/TCP))。
    -   `loadants_dns_request_duration_seconds`: DNS 请求处理耗时直方图 (标签: `protocol`, `query_type`)。
    -   `loadants_dns_request_errors_total`: DNS 请求处理错误总数 (标签: `error_type`)。
-   **缓存指标**:
    -   `loadants_cache_entries`: 当前 DNS 缓存条目数量。
    -   `loadants_cache_capacity`: DNS 缓存的最大容量。
    -   `loadants_cache_operations_total`: 缓存操作总数 (标签: `operation` (hit, miss, insert, evict, expire))。
    -   `loadants_cache_ttl_seconds`: DNS 缓存条目的 TTL 分布直方图 (标签: `source`)。
-   **DNS 查询详情**:
    -   `loadants_dns_query_type_total`: 按记录类型 (A, AAAA, MX 等) 统计的 DNS 查询总数 (标签: `type`)。
    -   `loadants_dns_response_codes_total`: 按响应代码 (RCODE) 统计的 DNS 响应总数 (标签: `rcode`)。
-   **上游解析器指标**:
    -   `loadants_upstream_requests_total`: 发送到上游 DoH 解析器的请求总数 (标签: `group`, `server`)。
    -   `loadants_upstream_errors_total`: 上游 DoH 解析器错误总数 (标签: `error_type`, `group`, `server`)。
    -   `loadants_upstream_duration_seconds`: 上游 DoH 查询耗时直方图 (标签: `group`, `server`)。
-   **路由指标**:
    -   `loadants_route_matches_total`: 路由规则匹配总数 (标签: `rule_type` (exact, wildcard, regex), `target_group`, `rule_source` (static, remote), `action` (block, forward))。
    -   `loadants_route_rules_count`: 当前活跃路由规则数量 (标签: `rule_type`, `rule_source`)。

这些丰富的指标支持对 Load Ants 性能和行为进行精细化监控和分析，有助于快速识别问题、优化配置并确保服务满足性能需求。

### API 端点

Load Ants 提供以下 HTTP API 端点：

#### DNS 服务端点

-   **UDP 和 TCP 端口 53** (或其他通过 `server.listen_udp` 和 `server.listen_tcp` 配置的端口)
    -   _描述_: 接收传统 DNS 查询的标准 DNS 端口。
    -   _协议_: DNS over UDP/TCP (RFC 1035)。
    -   _用途_: 标准 DNS 客户端、应用程序和系统通过这些端口发送查询。

#### 管理端点

默认监听在 `0.0.0.0:8080` (可通过 `health.listen` 配置修改)。

-   **GET /health**

    -   _描述_: 健康检查端点，用于服务监控和 Kubernetes liveness/readiness 探针。
    -   _返回_: 服务健康时返回 `200 OK` 和简单的 JSON 响应 `{"status":"healthy"}`。
    -   _用法_: `curl http://localhost:8080/health`

-   **GET /metrics**

    -   _描述_: Prometheus 指标端点，暴露性能和运行统计信息。
    -   _内容类型_: `text/plain; version=0.0.4; charset=utf-8`
    -   _用法_: `curl http://localhost:8080/metrics`

-   **POST /api/cache/refresh**
    -   _描述_: 清空 DNS 缓存的管理端点。
    -   _返回_: 表示成功或错误的 JSON 响应。
        -   成功: `200 OK` 与 `{"status":"success", "message":"DNS cache has been cleared"}`
        -   缓存未启用: `400 Bad Request` 与 `{"status":"error", "message":"Cache is not enabled"}`
        -   其他错误: `500 Internal Server Error` 与 `{"status":"error", "message":"Failed to clear cache"}`
    -   _用法_: `curl -X POST http://localhost:8080/api/cache/refresh`

API 端点遵循标准 HTTP 状态码。

### 应用场景

Load Ants 特别适合以下应用场景：

-   **个人用户/家庭网络**:
    -   提升隐私：将所有 DNS 查询通过 DoH 加密，防止 ISP 或网络中间人窥探。
    -   绕过封锁与审查：通过选择合适的 DoH 服务器，规避基于 DNS 的网络访问限制。
    -   广告与追踪器拦截：结合静态规则和远程阻止列表 (如从 `oisd.nl` 或其他来源获取)，有效拦截广告域名和追踪器。
    -   自定义解析：为特定域名指定特定的上游解析器 (例如，特定服务使用特定的 DNS)。
-   **开发者/测试环境**:
    -   本地 DoH 解析：方便地在本地测试需要 DoH 支持的应用。
    -   DNS 行为分析：通过日志和指标观察应用的 DNS 查询行为。
    -   灵活路由测试：快速搭建和测试复杂的 DNS 路由策略，包括基于远程列表的动态更新。
-   **企业/组织内部网络**:
    -   集中化 DNS 解析：统一管理内部网络的 DNS 查询，强制加密，提高网络安全基线。
    -   安全策略实施：通过规则阻止恶意域名、钓鱼网站、C&C 服务器等，可集成威胁情报源。
    -   内部域名解析：将内部域名解析请求路由到内部 DNS 服务器（如果内部 DNS 支持 DoH，或通过另一层非 DoH 代理）。
    -   合规性：记录和审计 DNS 查询（需自行配置日志收集和分析系统，Load Ants 提供结构化日志输出）。
-   **云原生环境 (Kubernetes, Docker Swarm)**:
    -   Sidecar 代理：作为 Sidecar 容器为集群内的其他应用提供 DoH 解析能力，无需修改应用本身。
    -   集群 DNS 服务：作为集群范围的 DNS 解析器（通常与 CoreDNS 等结合或作为其上游，以增强特定功能）。
    -   高性能 DNS 网关：为大规模应用提供高并发、低延迟的 DNS-to-DoH 转换和智能路由。

## 开源许可

[MIT 许可证](./LICENSE)

## 致谢

-   感谢所有为 Load Ants 项目做出贡献的开发者。
-   本项目的设计和实现受到了众多优秀的开源 DNS 工具和 DoH 实践的启发。
-   特别感谢 Rust 社区提供的强大工具和生态系统。
