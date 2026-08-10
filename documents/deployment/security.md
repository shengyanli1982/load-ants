# 安全最佳实践

将 Load Ants 部署在生产环境时，安全是首要考虑。遵循下面的实践可以加固应用，保护你的 DNS 服务和敏感数据。

## 1. 保护配置文件

`config.yaml` 文件包含整个服务的配置。若其中写入了密钥等敏感信息（我们不推荐这样做，见下一节），文件泄露会连带暴露这些信息，因此必须限制对该文件的访问权限。

**建议**：

- **最小权限原则**：将配置文件的所有者设置为运行 Load Ants 服务的用户（例如一个专门的 `load-ants` 用户；若使用上文 `systemd` 的默认配置，则为 `root`）。
- **设置文件权限**：移除所有其他用户的读取和写入权限。

```bash
# 假设运行服务的用户是 root
sudo chown root:root /etc/load-ants/config.yaml

# 设置权限为 600，只有所有者（root）可以读写
sudo chmod 600 /etc/load-ants/config.yaml
```

## 2. 使用部署系统管理密钥（避免写入 `config.yaml`）

以下配置项会写入密钥信息：

- `auth` 块中的 `token`
- `proxy` 链接中的密码
- `remote_rules` 中需要认证的 `url`

将这些信息以纯文本形式存储在 `config.yaml` 中存在安全风险。一旦配置文件泄露，这些密钥也会随之暴露。

> **重要**：当前版本的 Load Ants 不会在启动时自动展开 `config.yaml` 中的 `${VAR_NAME}` / `$VAR_NAME` 环境变量占位符。
>
> 推荐做法是：由部署系统在启动前渲染配置文件（模板 -> 实际 `config.yaml`），或使用 Secret/凭据管理能力注入运行环境。

### 示例

**不推荐的配置**：

```yaml
upstream_groups:
    - name: "private_doh"
      servers:
          - url: "https://private-doh.example.com/query"
            auth:
                type: "bearer"
                token: "MySuperSecretToken123" # 密钥硬编码
```

**推荐的配置（配置模板渲染）**：

1.  **创建 `config.yaml.tpl`（模板文件）**：

    ```yaml
    upstream_groups:
        - name: "private_doh"
          servers:
              - url: "https://private-doh.example.com/query"
                auth:
                    type: "bearer"
                    token: "${DOH_TOKEN}" # 使用环境变量占位符
    ```

2.  **在启动前渲染模板**（示例使用 `envsubst`）：
    - **直接运行（本机）**：

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
## 3. 保护 Admin API

Load Ants 的 Admin 服务器提供了以下运维端点：

| 端点                 | 方法   | 说明             |
| -------------------- | ------ | ---------------- |
| `/health/live`       | GET    | 存活检查         |
| `/health/ready`      | GET    | 就绪检查         |
| `/metrics`           | GET    | Prometheus 指标  |
| `/api/cache/clear`   | POST   | 清空缓存         |
| `/api/cache/dump`    | GET    | 导出缓存快照     |
| `/api/cache/restore` | POST   | 恢复缓存快照     |
| `/api/upstreams`     | GET    | 上游组健康状态   |
| `/api/routes`        | GET    | 当前生效路由规则 |
| `/api/info`          | GET    | 运行时摘要信息   |

配置 `admin.auth.token` 后，`/metrics` 与所有 `/api/*` 端点需要 Bearer token 才能访问；`/health/*` 端点不受保护。将这些端点暴露在公网上是极其危险的。

**建议**：

- **仅在本地监听**：确保 `admin` 的 `listen` 地址绑定到本地回环地址（`127.0.0.1` 或 `localhost`）。这是默认行为，但你需要确保没有错误地将其配置为 `0.0.0.0`。容器化部署（Docker/Kubernetes）时，需在容器配置中显式设置为 `0.0.0.0` 才能从容器外部访问。
    ```yaml
    admin:
        listen: "127.0.0.1:9000"
    ```
- **使用防火墙**：如果你必须从另一台机器访问 Admin API，请使用防火墙（如 `ufw`、`iptables`）限制只有特定的、可信的 IP 地址才能访问该端口。
    ```bash
    # 使用 ufw 只允许 192.168.1.100 访问 9000 端口（admin 默认端口）
    sudo ufw allow from 192.168.1.100 to any port 9000
    ```
- **使用反向代理**：将 Admin API 置于一个支持认证的反向代理（如 Nginx）之后，为该端点附加 HTTP Basic Auth 等认证机制。
- **启用内置 token 认证**：配置 `admin.auth.token` 后，访问 `/metrics` 与所有 `/api/*` 端点必须携带 Bearer token（`/health/*` 不受影响）。
    ```yaml
    admin:
        listen: "127.0.0.1:9000"
        auth:
            token: "your_secret_token"
    ```
    ```bash
    # 携带 Bearer token 访问
    curl -H "Authorization: Bearer your_secret_token" http://127.0.0.1:9000/api/info
    ```

## 4. 启用 TLS（HTTPS）

Load Ants 的 DNS over HTTPS（DoH）入站原生支持 TLS：在配置中同时指定 `server.tls_cert` 与 `server.tls_key`（PEM 格式），`listen_http` 端口即以 HTTPS 提供服务（rustls 实现）。

```yaml
server:
    listen_http: "0.0.0.0:8080"
    tls_cert: "/path/to/cert.pem"
    tls_key: "/path/to/key.pem"
```

此外，也可以使用反向代理处理 TLS，再将解密后的流量转发给 Load Ants。这种架构模式称为 TLS 终止代理：证书管理和 TLS 协议处理由代理承担，与核心应用逻辑解耦，是业界标准的做法。

**推荐的工具**：

- [➡️ **Caddy Server**](https://caddyserver.com/)：Caddy 是一个现代的 Web 服务器，以其自动化的 HTTPS 功能而闻名。它能自动从 Let's Encrypt 获取和续订 TLS 证书。
- [➡️ **Nginx**](https://nginx.org/)：Nginx 是一个高度可配置的 Web 服务器和反向代理。你需要手动配置证书（例如，使用 `certbot` 获取 Let's Encrypt 证书），但它提供了极大的灵活性。

**示例场景**：
假设你想对外提供一个 DoH 服务，监听在 `https://mydns.example.com`。

1.  **配置 Load Ants**：
    在 `config.yaml` 中，让 Load Ants 在本地的一个 HTTP 端口上监听。

    ```yaml
    server:
        listen_http: "127.0.0.1:8080"
        # ... 其他监听可以关闭或保留
    ```

2.  **配置反向代理（以 Caddy 为例）**：
    在你的 `Caddyfile` 中，添加如下配置：

    ```
    mydns.example.com {
        reverse_proxy 127.0.0.1:8080
    }
    ```

    Caddy 会自动处理 `mydns.example.com` 的 TLS 证书，并将传入的 HTTPS 请求转发到在本地 8080 端口上运行的 Load Ants 实例。

---

## 下一步

- [➡️ 了解如何监控服务](./monitoring.md)
- [➡️ 查看架构设计](../architecture/index.md)
- [➡️ 返回部署总览](./index.md)
