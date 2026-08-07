# 在 Docker Compose 上部署

`docker-compose` 用于定义和运行多容器 Docker 应用。单独启动 Load Ants 用 `docker run` 即可；当需要将它与其他服务（例如一个需要通过 Load Ants 解析域名来访问外部 API 的 Web 应用）一起部署时，`docker-compose` 能简化网络配置和管理。

本指南演示一个常见的场景：部署一个 Load Ants 容器和一个 `curl` 工具容器，并配置后者使用前者作为其 DNS 服务器。

## 先决条件

1.  **Docker 和 Docker Compose**：确保你的系统上已经安装了这两个工具。
2.  **配置文件**：准备好你的 `config.yaml` 文件。

## 场景描述

- **服务 A（`load-ants`）**：DNS 代理服务，监听 UDP 端口 5353。
- **服务 B（`my-app`）**：模拟应用（用 `curl` 容器代替），需要解析域名；所有 DNS 查询都发送到 `load-ants` 服务。

## 步骤一：创建项目目录结构

在一个你选择的目录下，创建以下文件和目录：

```
load-ants-compose/
├── docker-compose.yml
└── config/
    └── config.yaml
```

- `docker-compose.yml`: `docker-compose` 的核心定义文件。
- `config/config.yaml`: 你的 Load Ants 配置文件。

## 步骤二：配置 `config.yaml`

请确保你的 `config.yaml` 中 `server` 的监听地址是 `0.0.0.0`，否则无法接受来自其他容器的连接。

```yaml
server:
    listen_udp: "0.0.0.0:5353" # 使用一个非特权端口，如 5353
    # ... 其他配置
admin:
    listen: "0.0.0.0:9000" # 容器内必须显式覆盖默认的 127.0.0.1，否则同网络容器与端口映射均不可达
# ...
```

> **注意**：这里使用 5353 端口而不是标准的 53 端口，因为在某些系统上，53 端口已被本地的 DNS resolver（如 `systemd-resolved`）占用。在容器化环境中，推荐使用非特权端口。

## 步骤三：编写 `docker-compose.yml`

将以下内容粘贴到 `docker-compose.yml` 文件中：

```yaml
version: "3.8"

services:
    # 服务A: Load Ants
    load-ants:
        image: ghcr.io/shengyanli1982/load-ants-x64:latest # arm64 节点请改用 ghcr.io/shengyanli1982/load-ants-arm64:latest
        container_name: load-ants-server
        volumes:
            - ./config/config.yaml:/app/config.yaml
        # 不映射端口到主机，因为它只被内部服务使用
        # ports:
        #   - "5353:5353/udp"
        networks:
            ants-network:
                ipv4_address: 172.28.0.10 # 固定 IP，供 my-app 的 dns 配置指向

    # 服务B: 模拟的应用
    my-app:
        image: alpine/curl
        container_name: my-app-client
        # 使用 depends_on 确保 load-ants 容器先于 my-app 启动
        depends_on:
            - load-ants
        # 关键配置：将此容器的 DNS 服务器指向 load-ants 的固定 IP
        # 注意：dns 字段只接受 IP 地址（服务名无效），且容器内 127.0.0.1 上没有 DNS 监听
        dns:
            - 172.28.0.10
        # 保持容器运行以便我们进入
        command: ["sleep", "infinity"]
        networks:
            - ants-network

networks:
    ants-network:
        driver: bridge
        ipam:
            config:
                - subnet: 172.28.0.0/16
```

**配置解释**：

- **`services`**：定义了两个服务，`load-ants` 和 `my-app`。
- **`volumes`**：将本地的 `config.yaml` 文件挂载到 `load-ants` 容器的 `/app/config.yaml` 路径，这样容器就能读取到配置。
- **`dns`**：决定 `my-app` DNS 行为的关键配置。`dns` 字段只接受 IP 地址，不能写服务名；容器自身的 `127.0.0.1` 上也没有 DNS 监听。因此通过 `ipv4_address` 给 `load-ants` 分配了固定 IP `172.28.0.10`，并将 `my-app` 的 DNS 服务器指向它。`my-app` 容器内的所有 DNS 查询都会发送到 `load-ants` 容器。
- **`networks`**：创建了一个自定义的桥接网络 `ants-network`（子网 `172.28.0.0/16`），并让两个服务都连接到这个网络。两个服务因此可以相互通信，且 `load-ants` 拥有固定的 IP。如果需要将服务暴露给主机或其他外部网络，请务必参考[安全最佳实践](./security.md)中的建议。
- **`depends_on`**：确保容器的启动顺序，`my-app` 会在 `load-ants` 启动后才启动。

## 步骤四：启动和验证

1.  **启动服务**：
    在 `load-ants-compose` 目录下，运行以下命令。`-d` 参数表示在后台（detached mode）运行。

    ```bash
    docker-compose up -d
    ```

2.  **验证**：
    进入 `my-app` 容器，使用 `curl` 测试 DNS 解析是否通过 `load-ants`。
    ```bash
    docker exec -it my-app-client sh
    ```
    进入容器后，执行一个 `curl` 命令：
    ```sh
    # -v 参数会显示详细的连接信息，包括 DNS 解析过程
    curl -v https://www.google.com
    ```
    同时，查看 `load-ants` 容器的日志，可以看到来自 `my-app` 容器的 DNS 查询记录：
    ```bash
    docker logs -f load-ants-server
    ```

这个例子展示了 `docker-compose` 的声明式能力：只需几行 YAML，即可构建一个包含自定义 DNS 解析逻辑的多服务应用环境。

---

## 下一步

- [➡️ 了解安全注意事项](./security.md)
- [➡️ 了解如何监控服务](./monitoring.md)
- [➡️ 返回部署总览](./index.md)
