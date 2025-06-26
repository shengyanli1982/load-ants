# Docker 部署

使用 Docker 是部署和运行 Load Ants 最推荐的方式之一。它提供了一个干净、隔离的环境，无需在你的主机上直接安装任何依赖。

本指南将引导你使用 `docker run` 命令启动一个独立的 Load Ants 容器。

### 步骤一：创建配置目录

首先，在你的主机上创建一个目录，用于存放 `config.yaml`。这样可以让你在不重建 Docker 镜像的情况下轻松修改配置。

```bash
mkdir -p ./load-ants-config
```

### 步骤二：准备配置文件

将你的 `config.yaml` 文件放入刚刚创建的 `load-ants-config` 目录中。

如果你是初次使用，可以从项目中复制 `config.default.yaml` 并重命名。

```bash
# 假设你已获取 config.default.yaml
cp config.default.yaml ./load-ants-config/config.yaml
```

然后，使用你喜欢的文本编辑器修改 `./load-ants-config/config.yaml`。确保 `server` 和 `admin` 的监听地址设置为 `0.0.0.0`，以便从容器外部访问。

```yaml
# config.yaml
server:
    listen_udp: "0.0.0.0:53"
    listen_tcp: "0.0.0.0:53"
admin:
    listen: "0.0.0.0:9000"
```

### 步骤三：运行 Load Ants 容器

打开终端，运行以下命令来启动 Load Ants 容器：

```bash
docker run -d \
  --name load-ants \
  -p 53:53/udp \
  -p 53:53/tcp \
  -p 9000:9000/tcp \
  -v $(pwd)/load-ants-config:/app/config \
  --restart unless-stopped \
  ghcr.io/shengyanli1982/load-ants-<arch>:latest -c /app/config/config.yaml
```

**命令解释**:

-   `-d`: 在后台（detached mode）运行容器。
-   `--name load-ants`: 为容器指定一个易于记忆的名称。
-   `-p 53:53/udp -p 53:53/tcp`: 将主机的 53 端口（DNS 标准端口）的 UDP 和 TCP 流量映射到容器的 53 端口。
-   `-p 9000:9000/tcp`: 将主机的 9000 端口映射到容器的 `admin` 服务端口。
-   `-v $(pwd)/load-ants-config:/app/config`: **非常重要**。将当前目录下的 `load-ants-config` 目录挂载到容器内的 `/app/config` 目录。这使得容器可以读取到你的配置文件。
-   `--restart unless-stopped`: 配置容器在退出时总是自动重启，除非它被手动停止。这对于保证服务的持续运行很有用。
-   `ghcr.io/shengyanli1982/load-ants-<arch>:latest`: 要使用的 Docker 镜像。推荐使用官方提供的最新镜像。
-   `-c /app/config/config.yaml`: 指定容器内配置文件的路径。

### 步骤四：验证服务

容器启动后，你可以通过以下方式进行测试：

1.  **测试 DNS 解析**:
    使用 `dig` 或 `nslookup` 工具向你的主机 IP（例如 `127.0.0.1`）发送查询。

    ```bash
    dig @127.0.0.1 example.com
    ```

    如果配置正确，你应该能收到来自上游的 DNS 响应。

2.  **查看日志**:
    检查 Load Ants 的运行日志以了解其工作状态或排查问题。

    ```bash
    docker logs load-ants
    ```

3.  **测试管理端口**:
    你可以访问 `http://127.0.0.1:9000/metrics` 来查看 Prometheus 指标，确认 `admin` 服务也在正常工作。

### 停止和管理容器

-   **停止容器**:
    ```bash
    docker stop load-ants
    ```
-   **重新启动容器**:
    ```bash
    docker start load-ants
    ```
-   **移除容器**:
    ```bash
    # 必须先停止容器才能移除
    docker stop load-ants
    docker rm load-ants
    ```

---

### 下一步

-   [➡️ 尝试用 Docker Compose 部署](../deployment/docker-compose.md)
-   [➡️ 学习核心概念](../concepts/index.md)
-   [➡️ 查阅所有配置选项](../configuration/index.md)
