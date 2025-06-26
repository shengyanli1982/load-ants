# 在 Docker Compose 上部署

`docker-compose` 是一个用于定义和运行多容器 Docker 应用程序的强大工具。虽然你可以直接使用 `docker run` 来启动 Load Ants，但当你需要将它与其它服务（例如，一个需要通过 Load Ants 解析域名来访问外部 API 的 Web 应用）一起部署时，`docker-compose` 能极大地简化网络配置和管理。

本指南将演示一个常见的场景：部署一个 `Load Ants` 容器和一个 `curl` 工具容器，并配置后者使用前者作为其 DNS 服务器。

### 先决条件

1.  **Docker 和 Docker Compose**: 确保你的系统上已经安装了这两个工具。
2.  **配置文件**: 准备好你的 `config.yaml` 文件。

### 场景描述

-   **服务 A (`load-ants`)**: 我们的 DNS 代理，它将监听 UDP 端口 5353。
-   **服务 B (`my-app`)**: 一个模拟应用（我们用 `curl` 容器代替），它需要解析域名。我们将配置它把所有 DNS 查询发送给 `load-ants` 服务。

### 步骤一：创建项目目录结构

在一个你选择的目录下，创建以下文件和目录：

```
load-ants-compose/
├── docker-compose.yml
└── config/
    └── config.yaml
```

-   `docker-compose.yml`: `docker-compose` 的核心定义文件。
-   `config/config.yaml`: 你的 Load Ants 配置文件。

### 步骤二：配置 `config.yaml`

为了在 Docker 环境中工作，请确保你的 `config.yaml` 中 `server` 的监听地址是 `0.0.0.0`，这样它才能接受来自其他容器的连接。

```yaml
server:
    listen_udp: "0.0.0.0:5353" # 使用一个非特权端口，如 5353
    # ... 其他配置
# ...
```

> **注意**: 我们在这里使用 5353 端口而不是标准的 53 端口，因为在某些系统上，53 端口可能已被本地的 DNS resolver（如 `systemd-resolved`）占用。在容器化环境中，使用非特权端口是更简单和推荐的做法。

### 步骤三：编写 `docker-compose.yml`

这是此设置的核心。将以下内容粘贴到 `docker-compose.yml` 文件中：

```yaml
version: "3.8"

services:
    # 服务A: Load Ants
    load-ants:
        image: ghcr.io/shengyanli1982/load-ants-<arch>:latest
        container_name: load-ants-server
        volumes:
            - ./config/config.yaml:/app/config.yaml
        # 不映射端口到主机，因为它只被内部服务使用
        # ports:
        #   - "5353:5353/udp"
        networks:
            - ants-network

    # 服务B: 模拟的应用
    my-app:
        image: alpine/curl
        container_name: my-app-client
        # 使用 depends_on 确保 load-ants 容器先于 my-app 启动
        depends_on:
            - load-ants
        # 关键配置：将此容器的 DNS 服务器指向 load-ants 服务
        dns:
            - 127.0.0.1 # 作为备用
            - load-ants # Docker 会自动将服务名 'load-ants' 解析为其容器的 IP 地址
        # 保持容器运行以便我们进入
        command: ["sleep", "infinity"]
        networks:
            - ants-network

networks:
    ants-network:
        driver: bridge
```

**配置解释**:

-   **`services`**: 定义了两个服务，`load-ants` 和 `my-app`。
-   **`volumes`**: 我们将本地的 `config.yaml` 文件挂载到 `load-ants` 容器的 `/app/config.yaml` 路径，这样容器就能读取到我们的配置。
-   **`dns`**: 这是最关键的部分。我们为 `my-app` 服务设置了 DNS 服务器。Docker 的内置 DNS 服务会将服务名 `load-ants` 解析为 `load-ants` 容器在 `ants-network` 网络中的内部 IP 地址。这意味着 `my-app` 容器内的任何 DNS 查询都会被发送到 `load-ants` 容器。
-   **`networks`**: 我们创建了一个自定义的桥接网络 `ants-network`，并让两个服务都连接到这个网络。这能确保它们之间可以相互通信，并且拥有可预测的 DNS 名称。如果需要将服务暴露给主机或其他外部网络，请务必参考[安全最佳实践](./security.md)中的建议。
-   **`depends_on`**: 确保了容器的启动顺序，`my-app` 会在 `load-ants` 成功启动后才启动。

### 步骤四：启动和验证

1.  **启动服务**:
    在 `load-ants-compose` 目录下，运行以下命令。`-d` 参数表示在后台（detached mode）运行。

    ```bash
    docker-compose up -d
    ```

2.  **验证**:
    我们可以进入 `my-app` 容器，并使用 `curl` 来测试 DNS 解析是否通过 `load-ants`。
    ```bash
    docker exec -it my-app-client sh
    ```
    进入容器后，执行一个 `curl` 命令：
    ```sh
    # -v 参数会显示详细的连接信息，包括 DNS 解析过程
    curl -v https://www.google.com
    ```
    同时，你可以查看 `load-ants` 容器的日志，你应该能看到来自 `my-app` 容器的 DNS 查询记录。
    ```bash
    docker logs -f load-ants-server
    ```

这个例子展示了 `docker-compose` 的强大之处：只需几行声明式的 YAML，我们就构建了一个包含自定义 DNS 解析逻辑的多服务应用环境。
