# 安全最佳实践

将 Load Ants 部署在生产环境时，安全性是首要考虑的因素。遵循以下最佳实践可以帮助你加固应用，保护你的 DNS 服务和敏感数据免受威胁。

### 1. 保护配置文件

`config.yaml` 文件包含了你整个服务的配置，甚至可能包含敏感信息（尽管我们不推荐这样做，见下一节）。限制对该文件的访问权限至关重要。

**建议**:

- **最小权限原则**: 将配置文件的所有者设置为运行 Load Ants 服务的用户（例如，一个专门的 `load-ants` 用户，或 `root` 如果你使用 `systemd` 的默认配置）。
- **设置文件权限**: 移除所有其他用户的读取和写入权限。

```bash
# 假设运行服务的用户是 root
sudo chown root:root /etc/load-ants/config.yaml

# 设置权限为 600，只有所有者（root）可以读写
sudo chmod 600 /etc/load-ants/config.yaml
```

### 2. 使用部署系统管理密钥（避免写入 `config.yaml`）

你的配置文件中可能需要定义一些密钥信息，例如：

- `auth` 块中的 `token`
- `proxy` 链接中的密码
- `rules.remote` 中需要认证的 `url`

将这些信息以纯文本形式存储在 `config.yaml` 中存在安全风险。一旦配置文件泄露，这些密钥也会随之暴露。

> **重要**：当前版本的 Load Ants 不会在启动时自动展开 `config.yaml` 中的 `${VAR_NAME}` / `$VAR_NAME` 环境变量占位符。
>
> 推荐做法是：由部署系统在启动前渲染配置文件（模板 -> 实际 `config.yaml`），或使用 Secret/凭据管理能力注入运行环境。

#### 示例

**不推荐的配置**:

```yaml
upstreams:
    - name: "private_doh"
      protocol: "doh"
      policy: "roundrobin"
      endpoints:
          - url: "https://private-doh.example.com/query"
            auth:
                type: "bearer"
                token: "MySuperSecretToken123" # 密钥硬编码
```

**推荐的配置（配置模板渲染）**:

1.  **创建 `config.yaml.tpl`（模板文件）**:

    ```yaml
    upstreams:
        - name: "private_doh"
          protocol: "doh"
          policy: "roundrobin"
          endpoints:
              - url: "https://private-doh.example.com/query"
                auth:
                    type: "bearer"
                    token: "${DOH_TOKEN}" # 使用环境变量占位符
    ```

2.  **在启动前渲染模板**（示例使用 `envsubst`）：
    - **直接运行（本机）**:

        ```bash
        export DOH_TOKEN="MySuperSecretToken123"
        envsubst < ./config.yaml.tpl > ./config.yaml
        ./loadants -c ./config.yaml
        ```

    - **对于 `systemd` 服务**：用 `EnvironmentFile` 管理密钥，并在启动前渲染配置：

        ```ini
        [Service]
        EnvironmentFile=/etc/load-ants/load-ants.env
        ExecStartPre=/bin/sh -lc 'envsubst < /etc/load-ants/config.yaml.tpl > /etc/load-ants/config.yaml'
        ExecStart=/usr/local/bin/load-ants/loadants -c /etc/load-ants/config.yaml
        ```

        然后在 `/etc/load-ants/load-ants.env` 中写入（注意权限控制）：

        ```bash
        DOH_TOKEN=MySuperSecretToken123
        ```

<a id="3-保护-admin-api"></a>

### 3. 保护 Admin API

Load Ants 的 `admin` 服务提供了运维端点（例如 `/health`、`/metrics`，以及用于清空缓存的 `POST /api/cache/refresh`）。将它暴露在公网上是极其危险的。

**建议**:

- **仅在本地监听**: 确保 `admin` 的 `listen` 地址绑定到本地回环地址 (`127.0.0.1` 或 `localhost`)。这是默认行为，但你需要确保没有错误地将其配置为 `0.0.0.0`。
    ```yaml
    admin:
        listen: "127.0.0.1:9000"
    ```
- **使用防火墙**: 如果你必须从另一台机器访问 Admin API，请使用防火墙（如 `ufw`, `iptables`）来限制只有特定的、可信的 IP 地址才能访问该端口。
    ```bash
    # 使用 ufw 只允许 192.168.1.100 访问 9000 端口
    sudo ufw allow from 192.168.1.100 to any port 9000
    ```
- **使用反向代理**: 更安全的方式是，将 Admin API 置于一个支持认证的反向代理（如 Nginx）之后。你可以为该 API 端点设置 HTTP Basic Auth 或其他更强的认证机制。

### 4. 启用 TLS (HTTPS)

Load Ants 本身不直接处理 TLS 证书的配置和终止。推荐的做法是使用一个专门的、经过安全考验的反向代理来处理 TLS，然后将解密后的流量转发给 Load Ants。

这种架构模式被称为"TLS 终止代理"，它将复杂的证书管理和 TLS 协议处理与核心应用逻辑解耦，是业界标准的做法。

**推荐的工具**:

- [➡️ **Caddy Server**](https://caddyserver.com/): Caddy 是一个现代的 Web 服务器，以其自动化的 HTTPS 功能而闻名。它能自动从 Let's Encrypt 获取和续订 TLS 证书，配置极其简单。
- [➡️ **Nginx**](https://nginx.org/): Nginx 是一个功能强大且高度可配置的 Web 服务器和反向代理。你需要手动配置证书（例如，使用 `certbot` 获取 Let's Encrypt 证书），但它提供了极大的灵活性。

**示例场景**:
假设你想对外提供一个 DoH (DNS-over-HTTPS) 服务，监听在 `https://mydns.example.com`。

1.  **配置 Load Ants**:
    在 `config.yaml` 中，让 Load Ants 在本地的一个 HTTP 端口上监听。

    ```yaml
    listeners:
        udp: "127.0.0.1:53"
        tcp: "127.0.0.1:53"
        doh: "127.0.0.1:8080"
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

- [➡️ 了解如何监控服务](./monitoring.md)
- [➡️ 查看架构设计](../architecture/index.md)
- [➡️ 返回部署总览](./index.md)
