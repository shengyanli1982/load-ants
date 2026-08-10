# Docker 部署

使用 Docker 是部署和运行 Load Ants 的推荐方式之一：容器提供干净、隔离的运行环境，无需在主机上安装依赖。

本指南演示如何使用 `docker run` 启动一个独立的 Load Ants 容器。

## 步骤一：创建配置目录

首先，在主机上创建一个目录，用于存放 `config.yaml`。这样无需重建 Docker 镜像即可修改配置。

```bash
mkdir -p ./load-ants-config
```

## 步骤二：准备配置文件

将你的 `config.yaml` 文件放入刚刚创建的 `load-ants-config` 目录中。

初次使用时，可以从项目中复制 `config.default.yaml` 并重命名。

```bash
# 假设你已获取 config.default.yaml
cp config.default.yaml ./load-ants-config/config.yaml
```

然后，使用文本编辑器修改 `./load-ants-config/config.yaml`。`admin.listen` 默认仅绑定 `127.0.0.1:9000`，容器场景下必须显式将 `server` 和 `admin` 的监听地址设置为 `0.0.0.0`，才能从容器外部访问。

```yaml
# config.yaml
server:
    listen_udp: "0.0.0.0:53"
    listen_tcp: "0.0.0.0:53"
admin:
    listen: "0.0.0.0:9000"
```

## 步骤三：运行 Load Ants 容器

打开终端，运行以下命令启动 Load Ants 容器：

```bash
docker run -d \
  --name load-ants \
  -p 53:53/udp \
  -p 53:53/tcp \
  -p 9000:9000/tcp \
  -v $(pwd)/load-ants-config:/app/config \
  --restart unless-stopped \
  ghcr.io/shengyanli1982/load-ants-x64:latest -c /app/config/config.yaml
```

> **可选**：如果配置了 `server.listen_http` 提供 DNS over HTTPS（DoH）服务，请在命令中追加端口映射 `-p 8080:8080/tcp`（DoH 默认端口）。

**命令解释**：

- `-d`：在后台（detached mode）运行容器。
- `--name load-ants`：为容器指定一个易于记忆的名称。
- `-p 53:53/udp -p 53:53/tcp`：将主机的 53 端口（DNS 标准端口）的 UDP 和 TCP 流量映射到容器的 53 端口。
- `-p 9000:9000/tcp`：将主机的 9000 端口映射到容器内 Admin 服务器的端口。
- `-v $(pwd)/load-ants-config:/app/config`：**非常重要**。将当前目录下的 `load-ants-config` 目录挂载到容器内的 `/app/config` 目录，容器会从这里读取你的配置文件。
- `--restart unless-stopped`：容器退出后自动重启（手动停止除外），保证服务持续运行。
- `ghcr.io/shengyanli1982/load-ants-x64:latest`：要使用的 Docker 镜像。arm64 节点请改用 `ghcr.io/shengyanli1982/load-ants-arm64:latest`。
- `-c /app/config/config.yaml`：指定容器内配置文件的路径。

> **提示**：官方镜像内置了 HEALTHCHECK（定期访问 `http://localhost:9000/health/live`）。服务异常时，`docker ps` 中的容器状态会被标记为 `unhealthy`。

## 步骤四：验证服务

容器启动后，按下面的方法测试：

1.  **测试 DNS 解析**：
    使用 `dig` 或 `nslookup` 工具向你的主机 IP（例如 `127.0.0.1`）发送查询。

    ```bash
    dig @127.0.0.1 example.com
    ```

    配置正确时，命令返回来自上游的 DNS 响应。

2.  **查看日志**：
    检查 Load Ants 的运行日志，确认工作状态或排查问题。

    ```bash
    docker logs load-ants
    ```

3.  **测试管理端口**：
    访问 `http://127.0.0.1:9000/metrics` 查看 Prometheus 指标，确认 Admin 服务器正常工作。

## 停止和管理容器

- **停止容器**：
    ```bash
    docker stop load-ants
    ```
- **重新启动容器**：
    ```bash
    docker start load-ants
    ```
- **移除容器**：
    ```bash
    # 必须先停止容器才能移除
    docker stop load-ants
    docker rm load-ants
    ```

---

## 下一步

- [➡️ 尝试用 Docker Compose 部署](../deployment/docker-compose.md)
- [➡️ 学习核心概念](../concepts/index.md)
- [➡️ 查阅所有配置选项](../configuration/index.md)
