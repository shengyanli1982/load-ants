# 作为系统服务运行

在生产环境中，将 Load Ants 作为系统服务（Daemon）运行，由系统的服务管理器托管进程：开机自动启动、退出后按策略重启、运行状态可查询。

在现代 Linux 发行版中，`systemd` 是标准的服务管理器。本指南以 `systemd` 为例，介绍如何将 Load Ants 设置为系统服务。

## 先决条件

1.  **二进制文件**：你已经拥有编译好的 `loadants` 二进制文件。
2.  **配置文件**：你的 `config.yaml` 文件已经配置完毕。
3.  **权限**：你拥有 `sudo` 或 `root` 权限来创建服务文件和管理服务。

## 步骤一：准备文件和目录

首先，将二进制文件和配置文件放置到标准化的位置。

1.  **创建程序目录**：

    ```bash
    sudo mkdir -p /usr/local/bin/load-ants
    ```

2.  **复制二进制文件**：
    假设你的二进制文件名为 `loadants`，位于当前目录。

    ```bash
    sudo cp ./loadants /usr/local/bin/load-ants/
    ```

3.  **创建配置目录**：

    ```bash
    sudo mkdir -p /etc/load-ants
    ```

4.  **复制配置文件**：
    假设你的配置文件名为 `config.yaml`。
    ```bash
    sudo cp ./config.yaml /etc/load-ants/
    ```

## 步骤二：创建 `systemd` 服务单元文件

`systemd` 使用 `.service` 文件来定义如何管理一个服务。

1.  **创建服务文件**：
    使用文本编辑器（如 `nano` 或 `vim`）创建一个新的服务文件。

    ```bash
    sudo nano /etc/systemd/system/load-ants.service
    ```

2.  **填充服务文件内容**：
    将以下内容复制并粘贴到文件中。

    ```ini
    [Unit]
    Description=Load Ants DNS Proxy
    Documentation=https://shengyanli1982.github.io/load-ants/
    After=network.target

    [Service]
    Type=simple
    User=root
    Group=root
    ExecStart=/usr/local/bin/load-ants/loadants -c /etc/load-ants/config.yaml
    Restart=on-failure
    RestartSec=5s
    LimitNOFILE=65535

    [Install]
    WantedBy=multi-user.target
    ```

**参数解释**：

- `Description`：服务的简单描述。
- `After=network.target`：该服务在网络连接就绪之后启动。
- `User`/`Group`：指定运行服务的用户和组。使用 `root` 是因为监听 53 等特权端口需要 root 权限。如果你的监听端口大于 1024，可以改用非特权用户运行，避免以 root 身份暴露服务。更多信息请参考[安全最佳实践](./security.md)。
- `ExecStart`：定义启动服务的命令。这里使用 `-c` 参数明确指定配置文件的路径。
- `Restart=on-failure`：如果服务因非正常退出（例如崩溃）而停止，`systemd` 将尝试重启它。
- `LimitNOFILE`：提高进程可打开的文件描述符上限，满足高并发 DNS 服务对描述符数量的需求。
- `WantedBy=multi-user.target`：将服务安装到多用户运行级别，使其能在系统启动时自动运行。

## 步骤三：管理服务

你可以使用 `systemctl` 命令来控制 `load-ants` 服务了。

1.  **重新加载 `systemd` 配置**：
    每次创建或修改服务文件后，都需要运行此命令让 `systemd` 识别更改。

    ```bash
    sudo systemctl daemon-reload
    ```

2.  **启动服务**：

    ```bash
    sudo systemctl start load-ants
    ```

3.  **检查服务状态**：
    验证服务是否成功运行：

    ```bash
    sudo systemctl status load-ants
    ```

    服务正常运行时，输出包含 `active (running)` 的绿色状态提示。

4.  **查看服务日志**：
    如果服务启动失败，或者你想查看应用的实时输出，可以使用 `journalctl`。

    ```bash
    sudo journalctl -u load-ants -f
    ```

    （按 `Ctrl+C` 退出日志查看）

5.  **设置开机自启**：
    要让服务在服务器重启后自动运行，需要启用它。
    ```bash
    sudo systemctl enable load-ants
    ```

## 服务管理备忘单

- **停止服务**: `sudo systemctl stop load-ants`
- **重启服务**: `sudo systemctl restart load-ants`
- **禁用开机自启**: `sudo systemctl disable load-ants`

---

## 下一步

- [➡️ 了解安全注意事项](./security.md)
- [➡️ 了解如何监控服务](./monitoring.md)
- [➡️ 返回部署总览](./index.md)
