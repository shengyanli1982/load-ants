# 安全最佳实践

将 Load Ants 部署在生产环境时，安全性是首要考虑的因素。遵循以下最佳实践可以帮助你加固应用，保护你的 DNS 服务和敏感数据免受威胁。

### 1. 保护配置文件

`config.yaml` 文件包含了你整个服务的配置，甚至可能包含敏感信息（尽管我们不推荐这样做，见下一节）。限制对该文件的访问权限至关重要。

**建议**:

-   **最小权限原则**: 将配置文件的所有者设置为运行 Load Ants 服务的用户（例如，一个专门的 `load-ants` 用户，或 `root` 如果你使用 `systemd` 的默认配置）。
-   **设置文件权限**: 移除所有其他用户的读取和写入权限。

```bash
# 假设运行服务的用户是 root
sudo chown root:root /etc/load-ants/config.yaml

# 设置权限为 600，只有所有者（root）可以读写
sudo chmod 600 /etc/load-ants/config.yaml
```

### 2. 使用环境变量管理密钥

你的配置文件中可能需要定义一些密钥信息，例如：

-   `auth` 块中的 `token`
-   `proxy` 链接中的密码
-   `remote_rules` 中需要认证的 `url`

将这些信息以纯文本形式存储在 `config.yaml` 中存在安全风险。一旦配置文件泄露，这些密钥也会随之暴露。

Load Ants 支持使用环境变量来替代配置文件中的值，这是管理密钥信息的推荐方式。

**工作原理**:
你可以在 `config.yaml` 的值部分使用 `${VAR_NAME}` 或 `$VAR_NAME` 的语法，Load Ants 在启动时会自动用名为 `VAR_NAME` 的环境变量的值来替换它。

#### 示例

**不推荐的配置**:

```yaml
upstream_groups:
    - name: "private_doh"
      servers:
          - url: "https://private-doh.example.com/query"
            auth:
                type: "bearer"
                token: "MySuperSecretToken123" # 密钥硬编码
```

**推荐的配置**:

1.  **修改 `config.yaml`**:

    ```yaml
    upstream_groups:
        - name: "private_doh"
          servers:
              - url: "https://private-doh.example.com/query"
                auth:
                    type: "bearer"
                    token: "${DOH_TOKEN}" # 使用环境变量占位符
    ```

2.  **设置环境变量**:
    -   **直接运行**:
        ```bash
        export DOH_TOKEN="MySuperSecretToken123"
        ./load-ants -c config.yaml
        ```
    -   **对于 `systemd` 服务**:
        在你的 `/etc/systemd/system/load-ants.service` 文件中的 `[Service]` 部分，使用 `Environment` 指令或 `EnvironmentFile` 指令。
        ```ini
        [Service]
        # ...
        Environment="DOH_TOKEN=MySuperSecretToken123"
        ExecStart=/usr/local/bin/load-ants/load-ants -c /etc/load-ants/config.yaml
        # ...
        ```
        修改后记得运行 `sudo systemctl daemon-reload` 和 `sudo systemctl restart load-ants`。
    -   **对于 `docker-compose`**:
        在 `docker-compose.yml` 文件中为 `load-ants` 服务添加 `environment` 部分。
        ```yaml
        services:
            load-ants:
                # ...
                environment:
                    - DOH_TOKEN=MySuperSecretToken123
        ```

### 3. 保护 Admin API

Load Ants 的 `admin` 服务提供了强大的管理功能，如重载配置、清空缓存等。将它暴露在公网上是极其危险的。

**建议**:

-   **仅在本地监听**: 确保 `admin` 的 `listen` 地址绑定到本地回环地址 (`127.0.0.1` 或 `localhost`)。这是默认行为，但你需要确保没有错误地将其配置为 `0.0.0.0`。
    ```yaml
    admin:
        listen: "127.0.0.1:8080"
    ```
-   **使用防火墙**: 如果你必须从另一台机器访问 Admin API，请使用防火墙（如 `ufw`, `iptables`）来限制只有特定的、可信的 IP 地址才能访问该端口。
    ```bash
    # 使用 ufw 只允许 192.168.1.100 访问 8080 端口
    sudo ufw allow from 192.168.1.100 to any port 8080
    ```
-   **使用反向代理**: 更安全的方式是，将 Admin API 置于一个支持认证的反向代理（如 Nginx）之后。你可以为该 API 端点设置 HTTP Basic Auth 或其他更强的认证机制。

### 4. 启用 TLS (HTTPS)

Load Ants 本身不直接处理 TLS 证书的配置和终止。推荐的做法是使用一个专门的、经过安全考验的反向代理来处理 TLS，然后将解密后的流量转发给 Load Ants。

这种架构模式被称为"TLS 终止代理"，它将复杂的证书管理和 TLS 协议处理与核心应用逻辑解耦，是业界标准的做法。

**推荐的工具**:

-   [➡️ **Caddy Server**](https://caddyserver.com/): Caddy 是一个现代的 Web 服务器，以其自动化的 HTTPS 功能而闻名。它能自动从 Let's Encrypt 获取和续订 TLS 证书，配置极其简单。
-   [➡️ **Nginx**](https://nginx.org/): Nginx 是一个功能强大且高度可配置的 Web 服务器和反向代理。你需要手动配置证书（例如，使用 `certbot` 获取 Let's Encrypt 证书），但它提供了极大的灵活性。

**示例场景**:
假设你想对外提供一个 DoH (DNS-over-HTTPS) 服务，监听在 `https://mydns.example.com`。

1.  **配置 Load Ants**:
    在 `config.yaml` 中，让 Load Ants 在本地的一个 HTTP 端口上监听。

    ```yaml
    server:
        listen_http: "127.0.0.1:8080"
        # ... 其他监听可以关闭或保留
    ```

2.  **配置反向代理 (以 Caddy 为例)**:
    在你的 `Caddyfile` 中，添加如下配置：

    ```
    mydns.example.com {
        reverse_proxy 127.0.0.1:8080
    }
    ```

    Caddy 会自动处理 `mydns.example.com` 的 TLS 证书，并将所有传入的 HTTPS 请求安全地转发到在本地 8080 端口上运行的 Load Ants 实例。

---

### 下一步

-   [➡️ 了解如何监控服务](./monitoring.md)
-   [➡️ 查看架构设计](../architecture/index.md)
-   [➡️ 返回部署总览](./index.md)
