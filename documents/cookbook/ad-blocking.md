# 终极广告拦截

本教程将指导你如何使用 Load Ants 搭建一个强大的、全网络范围的广告、追踪器和恶意软件域名过滤系统。最终，你将拥有一个私人的、高性能的 DNS 服务，它可以保护你网络下的所有设备。

### 目标

-   订阅一个或多个社区维护的域名黑名单。
-   能够手动屏蔽或解封特定域名。
-   为正常的 DNS 查询选择一个或多个可信的、注重隐私的上游 DoH 服务器。
-   启用缓存以加速常见查询。
-   通过 Docker Compose 轻松部署和管理。

### 先决条件

1.  一台可以运行 Docker 和 Docker Compose 的主机（例如，一台 VPS、树莓派，或你的家用服务器）。
2.  对 Load Ants 的[基本配置](../configuration/index.md)有大致了解。

### 步骤一：项目结构

首先，创建一个项目目录：

```
load-ants-blocker/
├── docker-compose.yml
└── config/
    └── config.yaml
```

### 步骤二：`config.yaml` 配置

这是本配方的核心。我们将定义一个"干净"的上游组和一个"拦截"上游组，然后通过远程和静态规则来决定流量的走向。

将以下内容粘贴到 `config/config.yaml` 文件中：

```yaml
# ----------------------------------
# 日志配置
# ----------------------------------
log:
    level: "info" # 日常运行时使用 info，调试时可改为 debug
    format: "text"

# ----------------------------------
# 服务监听
# ----------------------------------
server:
    listen_udp: "0.0.0.0:53"
    listen_tcp: "0.0.0.0:53"
    listen_http: "0.0.0.0:5380" # 可选，用于 DoH

# ----------------------------------
# 缓存配置
# ----------------------------------
cache:
    enabled: true
    max_size: 100000 # 缓存最多 10 万条记录
    retention_period: 3600 # 缓存项保留 1 小时

# ----------------------------------
# 上游服务器组
# ----------------------------------
upstream_groups:
    - name: "clean_dns"
      strategy: "random"
      servers:
          - url: "https://dns.quad9.net/dns-query"
            weight: 10
          - url: "https://cloudflare-dns.com/dns-query"
            weight: 10

# ----------------------------------
# 路由规则
# ----------------------------------
static_rules:
    # 手动白名单 (优先级最高)
    # 如果某个域名被远程列表误杀，可以在这里放行
    - match: "exact"
      patterns:
          - "good-domain.com"
      action: "forward"
      target: "clean_dns"

    # 手动黑名单
    - match: "exact"
      patterns:
          - "very-bad-domain.com"
      action: "block"

    # 默认规则 (优先级最低)
    # 所有未被以上规则匹配的流量，都将转发到干净的上游
    - match: "wildcard"
      patterns: ["*"]
      action: "forward"
      target: "clean_dns"

remote_rules:
    # 订阅一个主流的广告/追踪器拦截列表
    # 你可以添加多个不同的列表
    - type: "url"
      url: "https://raw.githubusercontent.com/privacy-respecting-software/Blocky-Adlists/main/dns-hole-list.txt"
      format: "v2ray" # 该列表格式兼容 v2ray 格式
      action: "block"
      retry:
          attempts: 3
          delay: 1 # 指数回退，初始延迟1秒
```

**配置逻辑解读**:

1.  **`upstream_groups`**: 我们只定义了一个名为 `clean_dns` 的上游组，其中包含了两个广受好评的公共 DNS 解析服务。
2.  **`remote_rules`**: 我们订阅了一个远程维护的黑名单。Load Ants 会在启动和固定的时间间隔自动下载这个列表。所有在此列表中的域名都将被 `block` (拦截)。
3.  **`static_rules`**:
    -   我们设置的 `exact` 匹配规则优先级高于远程规则和 `wildcard` 规则。这给了我们最终的控制权。
    -   你可以通过向 `good-domain.com` 列表中添加域名来创建"白名单"，强制某个域名被解析。
    -   `very-bad-domain.com` 则是你的私人"黑名单"。
    -   最后，`wildcard` 规则 `"*"` 确保任何没有被拦截的域名都会被正常转发出去解析。

### 步骤三：`docker-compose.yml` 配置

这个配置文件将使部署变得异常简单。

将以下内容粘贴到 `docker-compose.yml` 文件中：

```yaml
version: "3.8"

services:
    load-ants-blocker:
        image: ghcr.io/shengyanli1982/load-ants-<arch>:latest
        container_name: load-ants-blocker
        restart: unless-stopped
        ports:
            # 将容器的 53 端口映射到主机的 53 端口
            - "53:53/udp"
            - "53:53/tcp"
            # 可选：如果你想使用 DoH，可以映射 HTTP 端口
            - "5380:5380/tcp"
        volumes:
            - ./config:/app/config
        cap_add:
            # 在某些系统上，监听低位端口需要此权限
            - NET_BIND_SERVICE
```

### 步骤四：启动和验证

1.  **启动服务**:
    在 `load-ants-blocker` 目录下，运行：

    ```bash
    docker-compose up -d
    ```

2.  **验证**:
    使用 `dig` 或 `nslookup` 工具，将 DNS 服务器指向你的 Docker 主机 IP。

    -   **测试一个正常域名**:

        ```bash
        dig @<your_docker_host_ip> www.google.com
        ```

        你应该能收到一个正常的 A 记录。

    -   **测试一个被拦截的域名**:
        从你订阅的[列表](https://raw.githubusercontent.com/privacy-respecting-software/Blocky-Adlists/main/dns-hole-list.txt)中找一个域名，例如 `101com.com`。

        ```bash
        dig @<your_docker_host_ip> 101com.com
        ```

        你应该会收到一个 `NXDOMAIN` 或 `0.0.0.0` 的响应，表示域名被成功拦截。

    -   **查看日志**:
        ```bash
        docker-compose logs -f
        ```
        你应该能在日志中看到查询、拦截和转发的记录。

### 步骤五：配置你的网络

现在你的 DNS 拦截服务已经成功运行，最后一步是让你的设备使用它。

-   **在路由器上配置**: 这是最推荐的方法。登录你的路由器管理页面，找到 DNS 设置，将主 DNS 服务器的 IP 地址改为你运行 Docker 的那台主机的 IP 地址。这样，连接到你 WiFi 的所有设备都会自动受到保护。
-   **在单个设备上配置**: 你也可以在你的电脑或手机的网络设置中手动指定 DNS 服务器。

恭喜！你现在拥有了一个属于自己的、功能强大的网络"净化器"。

---

### 下一步

-   [➡️ 回顾路由规则配置](../configuration/routing-rules.md)
-   [➡️ 尝试其他实例](./geo-unblocking.md)
-   [➡️ 返回实例总览](./index.md)
