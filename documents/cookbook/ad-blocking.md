# 终极广告拦截

本教程指导你使用 Load Ants 搭建一个覆盖全网络的广告、追踪器与恶意软件域名过滤系统。完成后，你将拥有一个私有的高性能 DNS 服务，为网络中的所有设备提供保护。

### 目标

- 订阅一个或多个社区维护的域名黑名单。
- 手动拦截或放行特定域名。
- 为正常的 DNS 查询选择一个或多个可信的上游解析器，推荐使用 DoH（DNS over HTTPS）以获得更好的隐私保护。
- 启用缓存以加速常见查询。
- 通过 Docker Compose 部署和管理。

### 先决条件

1.  一台可以运行 Docker 和 Docker Compose 的主机（例如，一台 VPS、树莓派，或你的家用服务器）。
2.  对 Load Ants 的[基本配置](../configuration/index.md)有大致了解。

### 步骤一：项目结构

首先，创建项目目录：

```bash
mkdir -p load-ants-blocker/config
```

目录结构如下：

```
load-ants-blocker/
├── docker-compose.yml
└── config/
    └── config.yaml
```

### 步骤二：`config.yaml` 配置

这是本配方的核心。定义一个名为 `clean_dns` 的“干净”上游组，再通过远程规则与静态规则决定流量的走向。

将以下内容粘贴到 `config/config.yaml` 文件中：

```yaml
# ----------------------------------
# 服务监听
# ----------------------------------
server:
    listen_udp: "0.0.0.0:53"
    listen_tcp: "0.0.0.0:53"
    listen_http: "0.0.0.0:5380" # 可选，用于 DoH

# ----------------------------------
# Admin 服务器
# ----------------------------------
admin:
    listen: "0.0.0.0:9000" # 默认为 127.0.0.1:9000；容器场景需绑定 0.0.0.0 才能通过端口映射从外部访问

# ----------------------------------
# 缓存配置
# ----------------------------------
cache:
    enabled: true
    max_size: 100000 # 缓存最多 10 万条记录
    min_ttl: 60 # 缓存条目最小 TTL（秒）
    max_ttl: 3600 # 缓存条目最大 TTL（秒）
    negative_ttl: 60 # 负面缓存 TTL（秒）

# ----------------------------------
# 上游组
# ----------------------------------
upstream_groups:
    - name: "clean_dns"
      strategy: "random" # weight 字段仅在 strategy 为 weighted 时生效，故此处不配置
      servers:
          - url: "https://dns.quad9.net/dns-query"
          - url: "https://cloudflare-dns.com/dns-query"

# ----------------------------------
# 路由规则
# ----------------------------------
static_rules:
    # 手动白名单（优先级最高）
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

    # 默认规则（优先级最低）
    # 所有未被以上规则匹配的流量，都将转发到干净的上游
    - match: "wildcard"
      patterns: ["*"]
      action: "forward"
      target: "clean_dns"

remote_rules:
    # 订阅一个主流的广告/追踪器拦截列表
    # 你可以添加多个不同的列表
    - type: "url"
      url: "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/reject-list.txt"
      format: "v2ray" # 该列表格式兼容 v2ray 格式
      action: "block"
      failure_policy: "strict" # 拉取失败且无可恢复快照时终止启动；可改为 lenient 允许降级启动
      retry:
          attempts: 3
          delay: 1 # 指数回退，初始延迟 1 秒
```

> 日志级别由 `RUST_LOG` 环境变量或 `--debug` 命令行参数控制（默认为 `loadants=info` 级别，`--debug` 启用调试日志）。

**配置逻辑解读**：

1.  **`upstream_groups`**：只定义了一个名为 `clean_dns` 的上游组，包含 Quad9 与 Cloudflare 两个公共 DoH 解析服务。
2.  **`remote_rules`**：订阅了一份远程维护的黑名单。当前版本的 Load Ants 会在**启动时**下载该列表，将列表中的域名作为 `block` 规则并入路由引擎，此后**定时刷新**，刷新间隔由 `reload_interval_secs` 控制（默认 3600 秒）。
    - **v2ray 列表解析语义**：列表中的裸域名（如 `example.com`）会被转换为通配符规则 `*.example.com`，即匹配该域名的所有子域名，但**不匹配** apex 域名 `example.com` 本身；如需同时拦截 apex 域名，列表中需提供 `full:example.com` 条目。
    - **`failure_policy`**：默认为 `strict`，即启动时列表拉取失败且没有可恢复的快照时，启动将终止；可改为 `lenient` 允许降级启动。
    - **快照机制**：快照默认开启，成功拉取的规则会保存到 `.load-ants/remote-rule-snapshots` 目录（相对工作目录），供远程源失败时恢复使用。请确保该目录对运行 Load Ants 的用户可写——若以非 root 用户运行，需检查目录权限。
3.  **`static_rules`**：
    - 得益于固定优先级契约（`exact` > `wildcard` > `regex` > `*`），我们设置的 `exact` `block` 规则会先于 `wildcard` 或 `regex` 的 `forward` 规则生效，手动拦截始终有效。
    - 将域名加入 `good-domain.com` 所在的 `patterns` 列表，即可创建“白名单”，强制放行该域名。
    - `very-bad-domain.com` 则是你的私人“黑名单”。
    - 最后，`wildcard` 规则 `"*"` 确保任何没有被拦截的域名都会被正常转发解析。

### 步骤三：`docker-compose.yml` 配置

下面的 Compose 文件声明了镜像、端口映射与配置挂载，一条命令即可完成部署。

将以下内容粘贴到 `docker-compose.yml` 文件中：

```yaml
version: "3.8"

services:
    load-ants-blocker:
        image: ghcr.io/shengyanli1982/load-ants-x64:latest # ARM64 主机请改用 load-ants-arm64
        container_name: load-ants-blocker
        restart: unless-stopped
        # 镜像默认读取 /app/config.yaml，需显式指定挂载进来的配置文件路径
        command: ["-c", "/app/config/config.yaml"]
        ports:
            # 将容器的 53 端口映射到主机的 53 端口
            - "53:53/udp"
            - "53:53/tcp"
            # 可选：如果你想使用 DoH，可以映射 HTTP 端口
            - "5380:5380/tcp"
            # 可选：如需从主机访问 Admin 服务器（健康检查、指标），映射 Admin 端口
            - "9000:9000/tcp"
        volumes:
            - ./config:/app/config
        cap_add:
            # 在某些系统上，监听低位端口需要此权限
            - NET_BIND_SERVICE
```

### 步骤四：启动和验证

1.  **启动服务**：
    在 `load-ants-blocker` 目录下，运行：

    ```bash
    docker-compose up -d
    ```

2.  **验证**：
    使用 `dig` 或 `nslookup` 工具，将 DNS 服务器指向你的 Docker 主机 IP。
    - **测试一个正常域名**：

        ```bash
        dig @<your_docker_host_ip> www.google.com
        ```

        预期返回一个正常的 A 记录。

    - **测试一个被拦截的域名**：
      从你订阅的[列表](https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/reject-list.txt)中找一个域名，例如 `www.101com.com`（注意：列表中的裸域名 `101com.com` 被转换为 `*.101com.com`，apex 域名本身不会被拦截，因此这里测试其子域名）。

        ```bash
        dig @<your_docker_host_ip> www.101com.com
        ```

        预期响应状态为 `REFUSED` 且没有应答记录，表示域名已被成功拦截。

    - **查看日志**：
        ```bash
        docker-compose logs -f
        ```
        日志中可以看到查询、拦截与转发的记录。

### 步骤五：配置你的网络

现在你的 DNS 拦截服务已经成功运行，最后一步是让你的设备使用它。

- **在路由器上配置**：这是推荐的方式。登录你的路由器管理页面，找到 DNS 设置，将主 DNS 服务器的 IP 地址改为运行 Docker 的那台主机的 IP 地址。这样，连接到 Wi-Fi 的所有设备都会自动受到保护。
- **在单个设备上配置**：也可以在电脑或手机的网络设置中手动指定 DNS 服务器。

恭喜！你现在拥有了一个属于自己的网络“净化器”。

---

### 下一步

- [➡️ 回顾路由规则配置](../configuration/routing-rules.md)
- [➡️ 尝试其他实例](./geo-unblocking.md)
- [➡️ 返回实例总览](./index.md)
