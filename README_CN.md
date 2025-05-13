<div align="center">
    <h1>LOAD ANTS</h1>
    <h4>🐜🐜🐜 一个轻量的 DNS 转发器，将 UDP/TCP 查询转换为 DoH。</h4></br>
    <img src="./images/logo.png" alt="logo" width="600">
</div>

## 项目介绍

**Load Ants** 是一个高性能、多功能的 DNS 代理服务，可将传统的 UDP/TCP DNS 查询转换为 DNS-over-HTTPS (DoH)。它充当使用标准 DNS 协议的客户端与现代安全 DoH 提供商之间的中介，提供增强的隐私、安全性和灵活的路由功能。

### 为什么选择 DNS-over-HTTPS？

传统的 DNS 查询以明文传输，使您的浏览历史容易受到潜在的监控、劫持或操纵。DoH 通过以下方式解决这些问题：

-   **加密 DNS 流量** - 防止网络中介窥探
-   **增强隐私** - 隐藏 DNS 查询，避免 ISP 和其他网络观察者获取
-   **提高安全性** - 降低 DNS 投毒和欺骗攻击的风险
-   **绕过审查** - 帮助规避基于 DNS 的封锁技术

## 主要特性

-   🔄 **协议转换**

    -   无缝将 UDP/53 和 TCP/53 DNS 请求转换为 DoH (RFC 8484)
    -   完全支持 GET 和 POST HTTP 方法
    -   处理多种内容格式，包括 `application/dns-message` 和 `application/dns-json`

-   🧠 **智能路由**

    -   **灵活匹配** - 根据域名模式路由 DNS 查询：
        -   精确域名匹配
        -   通配符域名匹配（`*.example.com`）
        -   正则表达式域名匹配
    -   **自定义操作** - 为每次匹配定义处理方式：
        -   转发到特定上游 DoH 组
        -   阻止查询（返回 NXDOMAIN）

-   🌐 **灵活的上游管理**

    -   **分组** - 将 DoH 服务器组织成具有独立设置的逻辑组
    -   **负载均衡** - 为每个组配置均衡策略：
        -   轮询 (RR) - 在服务器之间平均分配
        -   加权轮询 (WRR) - 根据容量优先考虑服务器
        -   随机 - 非确定性选择，增强隐私
    -   **认证支持** - 与需要认证的私有 DoH 提供商进行安全通信：
        -   HTTP 基本认证
        -   Bearer 令牌认证
    -   **资源优化** - 所有上游组共享 HTTP 客户端池以提高效率

-   ⚡ **性能增强**

    -   **智能缓存** - 内置 DNS 缓存减少延迟和上游负载
    -   **连接池** - 复用 HTTP 连接提高效率
    -   **可调 TTL** - 配置缓存响应的最小和最大 TTL

-   🔁 **可靠性**

    -   **重试机制** - 自动重试失败的 DoH 请求，可配置尝试次数
    -   **自定义超时** - 微调连接和请求超时

-   ⚙️ **管理**
    -   **YAML 配置** - 简单、人类可读的配置
    -   **配置验证** - 启动时或测试模式下进行严格验证
    -   **健康检查端点** - 为运维团队提供监控集成
    -   **Prometheus 指标** - 通过 `/metrics` 端点提供全面的监控指标

## 架构

Load Ants 遵循模块化架构，具有以下关键组件：

-   **服务器**：接受传统 DNS 查询的 UDP/TCP 监听器
-   **路由器**：将域名与规则匹配以确定处理操作
-   **上游管理器**：处理与 DoH 服务器的通信，包括负载均衡和认证
-   **缓存**：存储 DNS 响应以提高性能并减少上游负载
-   **处理器**：通过协调其他组件处理 DNS 查询

![architecture](./images/architecture.png)

## Prometheus 指标

Load Ants 提供全面的 Prometheus 指标，用于监控服务的性能、健康状态和运行状况。这些指标通过 `/metrics` 端点暴露，可被 Prometheus 或其他兼容的监控系统抓取。

![metrics](./images/metrics.png)

### DNS 性能指标

-   **loadants_dns_requests_total** (计数器) - 代理处理的 DNS 请求总数，按协议(UDP/TCP)标记
-   **loadants_dns_request_duration_seconds** (直方图) - DNS 请求处理持续时间（秒），按协议和查询类型标记
-   **loadants_dns_request_errors_total** (计数器) - DNS 请求处理错误总数，按错误类型标记

### 缓存效率指标

-   **loadants_cache_entries** (仪表盘) - 当前 DNS 缓存条目数
-   **loadants_cache_capacity** (仪表盘) - DNS 缓存的最大容量
-   **loadants_cache_operations_total** (计数器) - 缓存操作总数，按操作类型标记（命中、未命中、插入、驱逐、过期）
-   **loadants_cache_ttl_seconds** (直方图) - DNS 缓存条目的 TTL 分布（秒）

### DNS 查询指标

-   **loadants_dns_query_type_total** (计数器) - 按记录类型(A, AAAA, MX 等)的 DNS 查询总数
-   **loadants_dns_response_codes_total** (计数器) - 按响应代码(RCODE)的 DNS 响应总数

### 上游解析器指标

-   **loadants_upstream_requests_total** (计数器) - 发送到上游 DoH 解析器的请求总数，按组和服务器标记
-   **loadants_upstream_errors_total** (计数器) - 上游 DoH 解析器错误总数，按错误类型、组和服务器标记
-   **loadants_upstream_duration_seconds** (直方图) - 上游 DoH 查询持续时间（秒），按组和服务器标记

### DNS 路由指标

-   **loadants_route_matches_total** (计数器) - 路由规则匹配总数，按规则类型（精确、通配符、正则表达式）和目标组标记
-   **loadants_route_rules_count** (仪表盘) - 当前活跃路由规则数，按规则类型（精确、通配符、正则表达式）标记

这些指标支持对 Load Ants 性能和行为进行详细监控和分析，使识别问题、优化配置和确保服务满足性能要求变得更加容易。

## API 端点

Load Ants 提供以下 HTTP API 端点用于 DNS 解析和服务监控：

### DNS 端点

-   **UDP 和 TCP 端口 53**
    -   _描述_: 接收传统 DNS 查询的标准 DNS 端口
    -   _协议_: DNS over UDP/TCP (RFC 1035)
    -   _用途_: 使用标准 DNS 解析的应用程序和系统将查询发送到这些端口

### 监控和健康检查端点

-   **GET /health**

    -   _描述_: 用于监控服务和 Kubernetes 探针的健康检查端点
    -   _返回_: 服务健康时返回 200 OK
    -   _用法_: `curl http://localhost:8080/health`

-   **GET /metrics**
    -   _描述_: 暴露性能和运行统计信息的 Prometheus 指标端点
    -   _内容类型_: text/plain
    -   _用法_: `curl http://localhost:8080/metrics`

这些端点遵循标准 HTTP 状态码：

-   200: 查询/操作成功
-   500: 处理过程中出现服务器错误

## 使用场景

Load Ants 非常适合以下场景：

-   **企业/内部网络**：集中 DNS 解析，强制加密，实施内部名称解析策略
-   **个人用户/开发者**：绕过 ISP DNS 限制/投毒，提高隐私，灵活控制特定域名解析
-   **云环境**：作为 sidecar 或独立服务提供 DNS 解析能力

## 安装

### 前提条件

-   Rust 工具链（用于从源代码构建）
-   管理员/root 权限（用于绑定到 53 端口）

### 从源代码构建

1. 克隆仓库：

    ```bash
    git clone https://github.com/yourusername/load-ants.git
    cd load-ants
    ```

2. 构建应用：

    ```bash
    cargo build --release
    ```

3. 编译后的二进制文件可以在 [releases](https://github.com/shengyanli1982/load-ants/releases) 页面下载。

### 使用 Docker

Docker 提供了一种简单的方式来运行 Load Ants，无需直接在系统上安装 Rust 或依赖项。

1. 为配置创建一个目录：

    ```bash
    mkdir -p ./load-ants-config
    ```

2. 创建配置文件：

    ```bash
    cp config.default.yaml ./load-ants-config/config.yaml
    # 编辑配置文件以满足您的需求
    ```

3. 将 Load Ants 作为 Docker 容器运行：

    ```bash
    docker run -d \
      --name load-ants \
      -p 53:53/udp \
      -p 53:53/tcp \
      -p 8080:8080 \
      -v $(pwd)/load-ants-config:/etc/load-ants \
      yourusername/load-ants:latest -c /etc/load-ants/config.yaml
    ```

4. 检查容器日志：

    ```bash
    docker logs load-ants
    ```

5. 停止容器：
    ```bash
    docker stop load-ants
    docker rm load-ants
    ```

### Kubernetes 部署

对于生产环境，Kubernetes 提供了扩展性、高可用性和更简便的管理。

1. 为配置创建 ConfigMap：

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
            # 添加其余配置...
    ```

2. 创建 Deployment：

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

3. 创建 Service：

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

4. 应用配置：

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

### 作为服务使用

#### Linux (systemd)

1. 创建服务文件 `/etc/systemd/system/load-ants.service`：

    ```ini
    [Unit]
    Description=Load Ants DNS to DoH Proxy
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
    # 编辑配置文件以满足您的需求
    ```

3. 启用并启动服务：
    ```bash
    systemctl enable load-ants
    systemctl start load-ants
    ```

## 使用方法

### 命令行选项

```
load-ants [OPTIONS]

选项:
    -c, --config <PATH>    配置文件路径（默认：./config.yaml）
    -t, --test             测试配置文件并退出
    -h, --help             打印帮助信息
    -V, --version          打印版本信息
```

### 示例

1. 基于模板创建配置文件：

    ```bash
    cp config.default.yaml config.yaml
    ```

2. 编辑配置文件以满足您的需求

3. 使用您的配置运行 Load Ants：

    ```bash
    sudo ./load-ants -c config.yaml
    ```

4. 通过将其用作 DNS 服务器来测试服务：
    ```bash
    dig @127.0.0.1 example.com
    ```

## 配置

Load Ants 使用 YAML 文件进行配置。以下是主要部分的说明：

### 服务器配置

```yaml
server:
    listen_udp: "0.0.0.0:53" # UDP 监听地址和端口
    listen_tcp: "0.0.0.0:53" # TCP 监听地址和端口
```

### 健康检查

```yaml
health:
    listen: "0.0.0.0:8080" # 健康检查服务器监听地址和端口
```

### 缓存设置

```yaml
cache:
    enabled: true
    max_size: 10000 # 最大条目数（10-1000000）
    min_ttl: 60 # 最小 TTL，单位秒（1-86400）
    max_ttl: 3600 # 最大 TTL，单位秒（1-86400）
```

### HTTP 客户端设置

```yaml
http_client:
    connect_timeout: 5 # 连接超时，单位秒（1-120）
    request_timeout: 10 # 请求超时，单位秒（1-1200）
    idle_timeout: 60 # 空闲连接超时，单位秒（5-1800）
    keepalive: 60 # TCP keepalive，单位秒（5-600）
    agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
```

### 上游 DoH 服务器组

```yaml
upstream_groups:
    - name: "google_public"
      strategy: "roundrobin" # 策略：roundrobin, weighted, random
      servers:
          - url: "https://dns.google/dns-query"
          - url: "https://8.8.4.4/dns-query"
            method: "get" # 可选：get 或 post，默认为 post
            content_type: "message" # 可选：message 或 json，默认为 message
      retry:
          attempts: 3 # 重试次数（1-100）
          delay: 1 # 初始延迟，单位秒（1-120）
      proxy: "http://user:pass@proxyserver:port" # 可选代理

    - name: "secure_dns"
      strategy: "weighted"
      servers:
          - url: "https://example-doh.com/dns-query"
            weight: 70 # 加权策略的权重（1-65535）
            auth:
                type: "bearer" # 认证类型：basic 或 bearer
                token: "YOUR_API_TOKEN" # bearer 认证的令牌
          - url: "https://another-doh.com/dns-query"
            weight: 30
            auth:
                type: "basic"
                username: "user"
                password: "pass"
```

### 路由规则

```yaml
routing_rules:
    # 阻止特定域名
    - match: "exact" # 匹配类型：exact, wildcard, regex
      pattern: "ads.example.com" # 要匹配的模式
      action: "block" # 动作：block 或 forward

    # 将内部域名路由到特定上游组
    - match: "wildcard"
      pattern: "*.internal.local"
      action: "forward"
      target: "internal_dns" # 目标上游组

    # 使用正则表达式进行模式匹配
    - match: "regex"
      pattern: "^ads-.*\\.example\\.com$"
      action: "forward"
      target: "adblock_dns"

    # 默认规则（捕获所有）
    - match: "wildcard"
      pattern: "*" # 匹配所有内容
      action: "forward"
      target: "google_public" # 默认上游组
```

## 许可证

[MIT 许可证](LICENSE)

## 致谢

-   感谢所有帮助塑造 Load Ants 的贡献者
-   受现代 DoH 实现和灵活 DNS 路由需求的启发
