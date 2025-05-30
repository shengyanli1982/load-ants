[English](./README.md) | 中文

<div align="center">
    <h1>LOAD ANTS</h1>
    <h4>🐜🐜🐜 轻量级 DNS 转发器，实现 UDP/TCP 查询到 DoH 的无缝转换</h4></br>
    <img src="./images/logo.png" alt="logo" width="600">
</div>

<p align="center">
  <a href="#项目介绍">简介</a>
  |
  <a href="#核心功能">核心功能</a>
  |
  <a href="#架构设计">架构</a>
  |
  <a href="#prometheus-监控指标">Prometheus 指标</a>
  |
  <a href="#api-端点">API 端点</a>
  |
  <a href="#应用场景">应用场景</a>
  |
  <a href="#配置详解">配置</a>
  |
  <a href="#安装部署">部署指南</a>
  |
  <a href="#使用指南">使用指南</a>
</p>

## 项目介绍

**Load Ants** 是一款高性能、多功能的 DNS 代理服务，能够将传统的 UDP/TCP DNS 查询转换为 DNS-over-HTTPS (DoH)。它作为使用标准 DNS 协议的客户端与现代安全 DoH 提供商之间的桥梁，提供增强的隐私保护、安全性和灵活的路由功能。

### 为什么选择 DNS-over-HTTPS？

传统 DNS 查询采用明文传输，这使您的浏览历史容易遭受监控、劫持或篡改。DoH 通过以下方式解决这些问题：

-   **DNS 流量加密** - 防止网络中间人窥探
-   **隐私增强** - 隐藏 DNS 查询内容，避免被运营商和其他网络观察者捕获
-   **安全性提升** - 有效降低 DNS 投毒和欺骗攻击的风险
-   **突破网络限制** - 帮助规避基于 DNS 的网络封锁技术

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

    -   **智能重试** - 自动重试失败的 DoH 请求，支持可配置的尝试次数
    -   **超时控制** - 精确调整连接和请求超时参数

-   ⚙️ **管理能力**
    -   **YAML 配置** - 简洁、易读的配置方式
    -   **配置校验** - 启动时或测试模式下进行严格配置验证
    -   **健康检查** - 为运维团队提供完整的监控集成接口
    -   **Prometheus 指标** - 通过 `/metrics` 端点提供全面的监控指标

## 架构设计

Load Ants 采用模块化架构设计，包含以下核心组件：

-   **服务器模块**：接收传统 DNS 查询的 UDP/TCP 监听器
-   **路由模块**：将域名与规则进行匹配，确定处理策略
-   **上游管理模块**：处理与 DoH 服务器的通信，实现负载均衡和认证
-   **缓存模块**：高效存储 DNS 响应，提升性能并减轻上游负载
-   **处理器模块**：协调各组件，完成 DNS 查询的全流程处理

![architecture](./images/architecture.png)

## Prometheus 监控指标

Load Ants 提供完整的 Prometheus 监控指标，用于实时监控服务性能、健康状态和运行情况。这些指标通过 `/metrics` 端点暴露，可被 Prometheus 或其他兼容的监控系统采集。

![metrics](./images/metrics.png)

### DNS 性能指标

-   **loadants_dns_requests_total** (计数器) - 代理处理的 DNS 请求总数，按协议(UDP/TCP)分类
-   **loadants_dns_request_duration_seconds** (直方图) - DNS 请求处理耗时（秒），按协议和查询类型分类
-   **loadants_dns_request_errors_total** (计数器) - DNS 请求处理错误总数，按错误类型分类

### 缓存效率指标

-   **loadants_cache_entries** (仪表盘) - 当前 DNS 缓存条目数量
-   **loadants_cache_capacity** (仪表盘) - DNS 缓存的最大容量上限
-   **loadants_cache_operations_total** (计数器) - 缓存操作总数，按操作类型分类（命中、未命中、插入、驱逐、过期）
-   **loadants_cache_ttl_seconds** (直方图) - DNS 缓存条目的 TTL 分布（秒），按 TTL 来源分类（原始、最小 TTL、调整后、负向缓存 TTL）
-   **loadants_negative_cache_hits_total** (计数器) - 负向缓存命中总数，用于跟踪负向缓存的效率

### DNS 查询指标

-   **loadants_dns_query_type_total** (计数器) - 按记录类型(A, AAAA, MX 等)统计的 DNS 查询总数
-   **loadants_dns_response_codes_total** (计数器) - 按响应代码(RCODE)统计的 DNS 响应总数

### 上游解析器指标

-   **loadants_upstream_requests_total** (计数器) - 发送到上游 DoH 解析器的请求总数，按组和服务器分类
-   **loadants_upstream_errors_total** (计数器) - 上游 DoH 解析器错误总数，按错误类型、组和服务器分类
-   **loadants_upstream_duration_seconds** (直方图) - 上游 DoH 查询耗时（秒），按组和服务器分类

### DNS 路由指标

-   **loadants_route_matches_total** (计数器) - 路由规则匹配总数，按规则类型（精确、通配符、正则表达式）和目标组分类
-   **loadants_route_rules_count** (仪表盘) - 当前活跃路由规则数量，按规则类型（精确、通配符、正则表达式）分类

这些丰富的指标支持对 Load Ants 性能和行为进行精细化监控和分析，有助于快速识别问题、优化配置并确保服务满足性能需求。

## API 端点

Load Ants 提供以下 HTTP API 端点，用于 DNS 解析和服务监控：

### DNS 端点

-   **UDP 和 TCP 端口 53**
    -   _描述_: 接收传统 DNS 查询的标准 DNS 端口
    -   _协议_: DNS over UDP/TCP (RFC 1035)
    -   _用途_: 使用标准 DNS 解析的应用程序和系统通过这些端口发送查询

### 监控和健康检查端点

-   **GET /health**

    -   _描述_: 用于监控服务和 Kubernetes 探针的健康检查端点
    -   _返回_: 服务健康时返回 200 OK
    -   _用法_: `curl http://localhost:8080/health`

-   **GET /metrics**

    -   _描述_: 暴露性能和运行统计信息的 Prometheus 指标端点
    -   _内容类型_: text/plain
    -   _用法_: `curl http://localhost:8080/metrics`

-   **POST /api/cache/refresh**
    -   _描述_: 清空 DNS 缓存的管理端点
    -   _返回_: 表示成功或错误的 JSON 响应
    -   _用法_: `curl -X POST http://localhost:8080/api/cache/refresh`
    -   _响应示例_: `{"status":"success","message":"DNS cache has been cleared"}`

这些端点遵循标准 HTTP 状态码：

-   200: 查询/操作成功
-   400: 请求错误（例如，当缓存未启用时）
-   500: 处理过程中发生服务器错误

## 应用场景

Load Ants 特别适合以下应用场景：

-   **企业/内部网络**：集中化 DNS 解析，强制流量加密，实施精细的内部名称解析策略
-   **个人用户/开发者**：绕过运营商 DNS 劫持/污染，提升隐私保护，精确控制特定域名解析
-   **云原生环境**：作为 sidecar 或独立服务，提供高性能的 DNS 解析能力

## 安装部署

### 环境要求

-   Rust 工具链（从源代码构建时需要）
-   管理员/root 权限（绑定 53 端口时需要）

### 从源代码构建

1. 克隆代码仓库：

    ```bash
    git clone https://github.com/yourusername/load-ants.git
    cd load-ants
    ```

2. 构建应用程序：

    ```bash
    cargo build --release
    ```

3. 编译后的二进制文件也可以直接从[发布页面](https://github.com/shengyanli1982/load-ants/releases)下载。

### 使用 Docker 部署

Docker 提供了一种简便的方式来运行 Load Ants，无需在系统上直接安装 Rust 或其依赖项。

1. 创建配置目录：

    ```bash
    mkdir -p ./load-ants-config
    ```

2. 准备配置文件：

    ```bash
    cp config.default.yaml ./load-ants-config/config.yaml
    # 编辑配置文件以满足您的实际需求
    ```

3. 以 Docker 容器方式运行 Load Ants：

    ```bash
    docker run -d \
      --name load-ants \
      -p 53:53/udp \
      -p 53:53/tcp \
      -p 8080:8080 \
      -v $(pwd)/load-ants-config:/etc/load-ants \
      yourusername/load-ants:latest -c /etc/load-ants/config.yaml
    ```

4. 查看容器日志：

    ```bash
    docker logs load-ants
    ```

5. 停止和移除容器：
    ```bash
    docker stop load-ants
    docker rm load-ants
    ```

### Kubernetes 部署方案

对于生产环境，Kubernetes 提供了更好的扩展性、高可用性和管理便捷性。

1. 创建配置 ConfigMap：

    ```yaml
    # configmap.yaml
    apiVersion: v1
    kind: ConfigMap
    metadata:
        name: load-ants-config
        namespace: dns
    data:
        config.yaml: |
            server:
              listen_udp: "0.0.0.0:53"
              listen_tcp: "0.0.0.0:53"
            health:
              listen: "0.0.0.0:8080"
            cache:
              enabled: true
              max_size: 10000
              min_ttl: 60
              max_ttl: 3600
              negative_ttl: 300
            # 添加其他必要配置...
    ```

2. 创建 Deployment 资源：

    ```yaml
    # deployment.yaml
    apiVersion: apps/v1
    kind: Deployment
    metadata:
        name: load-ants
        namespace: dns
        labels:
            app: load-ants
    spec:
        replicas: 2
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
                      image: yourusername/load-ants:latest
                      args: ["-c", "/etc/load-ants/config.yaml"]
                      ports:
                          - containerPort: 53
                            name: dns-udp
                            protocol: UDP
                          - containerPort: 53
                            name: dns-tcp
                            protocol: TCP
                          - containerPort: 8080
                            name: health
                      volumeMounts:
                          - name: config-volume
                            mountPath: /etc/load-ants
                      resources:
                          limits:
                              memory: "256Mi"
                              cpu: "500m"
                          requests:
                              memory: "128Mi"
                              cpu: "100m"
                      livenessProbe:
                          httpGet:
                              path: /health
                              port: 8080
                          initialDelaySeconds: 5
                          periodSeconds: 10
                volumes:
                    - name: config-volume
                      configMap:
                          name: load-ants-config
    ```

3. 创建 Service 资源：

    ```yaml
    # service.yaml
    apiVersion: v1
    kind: Service
    metadata:
        name: load-ants
        namespace: dns
    spec:
        selector:
            app: load-ants
        ports:
            - port: 53
              name: dns-udp
              protocol: UDP
              targetPort: 53
            - port: 53
              name: dns-tcp
              protocol: TCP
              targetPort: 53
        type: ClusterIP
    ```

4. 应用配置到集群：

    ```bash
    kubectl create namespace dns
    kubectl apply -f configmap.yaml
    kubectl apply -f deployment.yaml
    kubectl apply -f service.yaml
    ```

5. 检查部署状态：
    ```bash
    kubectl -n dns get pods
    kubectl -n dns get svc
    ```

### 作为系统服务使用

#### Linux (systemd)

1. 创建服务文件 `/etc/systemd/system/load-ants.service`：

    ```ini
    [Unit]
    Description=Load Ants DNS to DoH 代理服务
    After=network.target

    [Service]
    ExecStart=/path/to/load-ants -c /etc/load-ants/config.yaml
    Restart=on-failure
    User=root

    [Install]
    WantedBy=multi-user.target
    ```

2. 创建配置目录和文件：

    ```bash
    mkdir -p /etc/load-ants
    cp config.default.yaml /etc/load-ants/config.yaml
    # 根据实际需求编辑配置文件
    ```

3. 启用并启动服务：
    ```bash
    systemctl enable load-ants
    systemctl start load-ants
    ```

## 使用指南

### 命令行参数

```
load-ants [OPTIONS]

选项:
    -c, --config <PATH>    指定配置文件路径（默认：./config.yaml）
    -t, --test             测试配置文件有效性并退出
    -h, --help             显示帮助信息
    -V, --version          显示版本信息
```

### 使用示例

1. 基于默认模板创建配置文件：

    ```bash
    cp config.default.yaml config.yaml
    ```

2. 根据实际需求编辑配置文件

3. 启动 Load Ants 服务：

    ```bash
    sudo ./load-ants -c config.yaml
    ```

4. 测试服务是否正常工作：
    ```bash
    dig @127.0.0.1 example.com
    ```

## 配置详解

Load Ants 使用 YAML 格式的配置文件。以下是完整的配置选项参考：

### 服务器配置 (server)

| 参数        | 类型   | 默认值         | 描述                   | 有效范围            |
| ----------- | ------ | -------------- | ---------------------- | ------------------- |
| listen_udp  | 字符串 | "127.0.0.1:53" | UDP DNS 监听地址和端口 | 有效的 IP:端口 格式 |
| listen_tcp  | 字符串 | "127.0.0.1:53" | TCP DNS 监听地址和端口 | 有效的 IP:端口 格式 |
| tcp_timeout | 整数   | 10             | TCP 连接空闲超时（秒） | -                   |

### 健康检查配置 (health)

| 参数   | 类型   | 默认值           | 描述                       | 有效范围            |
| ------ | ------ | ---------------- | -------------------------- | ------------------- |
| listen | 字符串 | "127.0.0.1:8080" | 健康检查服务监听地址和端口 | 有效的 IP:端口 格式 |

### 缓存配置 (cache)

| 参数         | 类型   | 默认值 | 描述               | 有效范围   |
| ------------ | ------ | ------ | ------------------ | ---------- |
| enabled      | 布尔值 | true   | 是否启用缓存       | true/false |
| max_size     | 整数   | 10000  | 最大缓存条目数     | 10-1000000 |
| min_ttl      | 整数   | 60     | 最小 TTL（秒）     | 1-86400    |
| max_ttl      | 整数   | 3600   | 最大 TTL（秒）     | 1-86400    |
| negative_ttl | 整数   | 300    | 负向缓存 TTL（秒） | 1-86400    |

缓存配置允许精细调整 DNS 响应的缓存行为：

-   **enabled**：控制缓存功能的开关
-   **max_size**：缓存中可存储的 DNS 记录最大数量
-   **min_ttl**：正向响应的最小生存时间（会覆盖原始响应中更小的 TTL 值）
-   **max_ttl**：所有缓存条目的最大生存时间上限
-   **negative_ttl**：负向响应（如错误、不存在域名）的特定生存时间

负向缓存是一种重要的性能优化技术，它将 DNS 错误响应（如 NXDOMAIN 或 ServFail）缓存指定时间。这能有效防止对不存在或暂时无法解析的域名重复查询上游服务器，从而减少延迟并降低上游服务器负载。

### HTTP 客户端配置 (http_client)

| 参数            | 类型   | 默认值 | 描述                        | 有效范围   |
| --------------- | ------ | ------ | --------------------------- | ---------- |
| connect_timeout | 整数   | 5      | 连接超时（秒）              | 1-120      |
| request_timeout | 整数   | 10     | 请求超时（秒）              | 1-1200     |
| idle_timeout    | 整数   | 60     | 空闲连接超时（秒）（可选）  | 5-1800     |
| keepalive       | 整数   | 60     | TCP Keepalive（秒）（可选） | 5-600      |
| agent           | 字符串 | -      | HTTP 用户代理（可选）       | 非空字符串 |

### 上游 DoH 服务器组配置 (upstream_groups)

| 参数     | 类型   | 默认值 | 描述                     | 有效范围                           |
| -------- | ------ | ------ | ------------------------ | ---------------------------------- |
| name     | 字符串 | -      | 组名称                   | 非空字符串                         |
| strategy | 字符串 | -      | 负载均衡策略             | "roundrobin", "weighted", "random" |
| servers  | 数组   | -      | 服务器列表               | 至少包含一个服务器                 |
| retry    | 对象   | -      | 重试配置（可选）         | -                                  |
| proxy    | 字符串 | -      | HTTP/SOCKS5 代理（可选） | 有效的代理 URL                     |

#### 服务器配置 (servers)

| 参数         | 类型   | 默认值    | 描述                   | 有效范围                     |
| ------------ | ------ | --------- | ---------------------- | ---------------------------- |
| url          | 字符串 | -         | DoH 服务器 URL         | 有效的 HTTP(S) URL，包含路径 |
| weight       | 整数   | 1         | 权重（仅用于加权策略） | 1-65535                      |
| method       | 字符串 | "post"    | DoH 请求方法           | "get", "post"                |
| content_type | 字符串 | "message" | DoH 内容类型           | "message", "json"            |
| auth         | 对象   | -         | 认证配置（可选）       | -                            |

#### 认证配置 (auth)

| 参数     | 类型   | 默认值 | 描述                        | 有效范围          |
| -------- | ------ | ------ | --------------------------- | ----------------- |
| type     | 字符串 | -      | 认证类型                    | "basic", "bearer" |
| username | 字符串 | -      | 用户名（仅用于 basic 认证） | 非空字符串        |
| password | 字符串 | -      | 密码（仅用于 basic 认证）   | 非空字符串        |
| token    | 字符串 | -      | 令牌（仅用于 bearer 认证）  | 非空字符串        |

#### 重试配置 (retry)

| 参数     | 类型 | 默认值 | 描述           | 有效范围 |
| -------- | ---- | ------ | -------------- | -------- |
| attempts | 整数 | -      | 重试次数       | 1-100    |
| delay    | 整数 | -      | 初始延迟（秒） | 1-120    |

### 路由规则配置 (routing_rules)

| 参数     | 类型   | 默认值 | 描述                                          | 有效范围                     |
| -------- | ------ | ------ | --------------------------------------------- | ---------------------------- |
| match    | 字符串 | -      | 匹配类型                                      | "exact", "wildcard", "regex" |
| patterns | 数组   | -      | 匹配模式                                      | 非空字符串数组               |
| action   | 字符串 | -      | 路由动作                                      | "forward", "block"           |
| target   | 字符串 | -      | 目标上游组（当 action 为 forward 时必须提供） | 已定义的上游组名称           |

Load Ants 采用基于优先级的匹配系统进行 DNS 路由决策：

1. **精确匹配**（最高优先级）- 完全匹配完整域名（如 `example.com`）
2. **特定通配符匹配** - 使用通配符匹配特定域名模式（如 `*.example.com`）
3. **正则表达式匹配** - 使用正则表达式进行复杂匹配（如 `^(mail|audio)\\.google\\.com$`）
4. **全局通配符匹配**（最低优先级）- 使用通配规则（`*`）匹配任何域名

配置路由规则时，应充分考虑这个优先级顺序。全局通配符（`*`）通常应作为最后一条规则，作为其他规则都不匹配时的默认选项。

### 配置示例

```yaml
# Load Ants 配置示例

# 服务器监听设置
server:
    listen_udp: "0.0.0.0:53" # UDP 监听地址和端口
    listen_tcp: "0.0.0.0:53" # TCP 监听地址和端口

# 健康检查服务器设置
health:
    listen: "0.0.0.0:8080" # 健康检查服务器监听地址和端口

# 缓存设置
cache:
    enabled: true
    max_size: 10000 # 最大缓存条目数
    min_ttl: 60 # 缓存条目最小 TTL（秒）
    max_ttl: 3600 # 缓存条目最大 TTL（秒）
    negative_ttl: 300 # 负面缓存 TTL（秒），用于缓存错误响应

# HTTP 客户端设置
http_client:
    connect_timeout: 5 # 连接超时（秒）
    request_timeout: 10 # 请求超时（秒）
    idle_timeout: 60 # 空闲连接超时（秒）
    keepalive: 60 # TCP Keepalive（秒）
    agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"

# 上游 DoH 服务器组
upstream_groups:
    - name: "google_public"
      strategy: "roundrobin" # roundrobin, weighted, random
      servers:
          - url: "https://dns.google/dns-query"
          - url: "https://8.8.4.4/dns-query"
            method: "get"
            content_type: "message"
      retry:
          attempts: 3
          delay: 1
      proxy: "http://user:pass@proxyserver:port" # 可选的代理

    - name: "cloudflare_secure"
      strategy: "random"
      servers:
          - url: "https://cloudflare-dns.com/dns-query"
            method: "post"
          - url: "https://1.0.0.1/dns-query"
            method: "get"
            content_type: "json"

    - name: "nextdns_weighted"
      strategy: "weighted"
      servers:
          - url: "https://dns.nextdns.io/YOUR_CONFIG_ID"
            weight: 70
            auth:
                type: "bearer"
                token: "YOUR_API_KEY_OR_TOKEN"
          - url: "https://dns2.nextdns.io/YOUR_CONFIG_ID"
            weight: 30
      retry:
          attempts: 2
          delay: 2

# 路由规则（按顺序处理）
routing_rules:
    # 阻止特定域名
    - match: "exact"
      patterns: ["ads.example.com", "ads2.example.com"] # 支持多个模式的数组
      action: "block"

    # 将内部域名路由到内部解析器
    - match: "wildcard"
      patterns: ["*.corp.local", "*.corp.internal"] # 支持多个模式的数组
      action: "forward"
      target: "internal_doh"

    # 使用正则表达式进行模式匹配
    - match: "regex"
      patterns: ["^(video|audio)-cdn\\..+\\.com$"]
      action: "forward"
      target: "google_public"

    # 默认规则：将所有其他流量转发到 google_public
    - match: "wildcard"
      patterns: ["*"]
      action: "forward"
      target: "google_public"
```

## 开源许可

[MIT 许可证](LICENSE)

## 致谢

-   感谢所有为 Load Ants 项目做出贡献的开发者
-   本项目受现代 DoH 实现技术和灵活 DNS 路由需求的启发
