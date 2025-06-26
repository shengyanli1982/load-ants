# 快速上手

本指南将帮助你在 5 分钟内启动并运行 Load Ants。我们将使用最基础的配置，让你快速体验到核心功能。

### 环境要求

-   一台计算机（Windows、Linux 或 macOS）。
-   能够访问互联网。
-   一个文本编辑器（如 VS Code、Notepad++、Vim 等）。

---

### 第一步：下载预编译程序

访问项目的 [GitHub 发布页面](https://github.com/shengyanli1982/load-ants/releases)，根据你的操作系统下载最新的二进制文件。

例如，如果你使用的是 64 位的 Linux 系统，你应该下载名为 `load-ants-linux-amd64` 的文件。

下载后，建议将文件重命名为 `load-ants` (Windows 用户可以重命名为 `load-ants.exe`)，并将其放置在一个专属的文件夹中，方便管理。

### 第二步：创建配置文件

这是让 Load Ants 运行起来的关键。最简单的方式是**从默认配置开始**。

1.  **获取 `config.default.yaml`**:
    在项目的 [GitHub 仓库](https://github.com/shengyanli1982/load-ants) 中找到 `config.default.yaml` 文件，并将其下载到与 `load-ants` 程序相同的目录下。

2.  **重命名并编辑**:
    将 `config.default.yaml` 重命名为 `config.yaml`。

    ```bash
    # 假设 config.default.yaml 已下载到当前目录
    mv config.default.yaml config.yaml
    ```

    然后，用你的文本编辑器打开 `config.yaml`。对于快速上手，你暂时无需修改任何内容，默认配置已经包含了可以工作的基本设置。

> ✨ **提示**: 默认的 `config.yaml` 文件已经为你配置好了：
>
> 1.  让 Load Ants 在本地的 `53` 端口上监听 DNS 请求。
> 2.  定义了一个名为 `google_public` 的上游组，其中包含两个 Google DoH 服务器。
> 3.  设置了一条默认规则，将所有收到的请求都转发给 `google_public` 组处理。

### 第三步：运行 Load Ants

打开你的终端（在 Windows 上是 `命令提示符` 或 `PowerShell`）。

**对于 Linux / macOS 用户**:
你可能需要先给程序添加执行权限：

```bash
chmod +x ./load-ants
```

然后，使用 `sudo`（因为需要绑定特权端口 53）运行程序：

```bash
sudo ./load-ants -c ./config.yaml
```

**对于 Windows 用户**:
请**以管理员身份**打开命令提示符或 PowerShell，然后运行：

```powershell
.\load-ants.exe -c .\config.yaml
```

如果一切顺利，你将不会在终端看到任何错误信息，程序会在前台运行，并输出日志。

### 第四步：验证 DNS 解析

现在，Load Ants 已经在你的电脑上作为 DNS 服务器运行了。让我们来测试一下。

打开**一个新的**终端窗口，使用 `nslookup` 或 `dig` 工具向 `127.0.0.1`（也就是你的本机）发送一个 DNS 查询请求。

**使用 `nslookup` (Windows、Linux、macOS):**

```bash
nslookup example.com 127.0.0.1
```

你应该会看到类似下面的成功响应：

```
服务器:   localhost
Address:  127.0.0.1

非权威应答:
名称:    example.com
Addresses:  2606:2800:220:1:248:1893:25c8:1946
          93.184.216.34
```

**使用 `dig` (Linux、macOS):**

```bash
dig @127.0.0.1 example.com
```

你应该会看到类似下面的成功响应：

```
; <<>> DiG 9.16.1-Ubuntu <<>> @127.0.0.1 example.com
; (1 server found)
;; global options: +cmd
;; Got answer:
;; ->>HEADER<<- opcode: QUERY, status: NOERROR, id: 12345
;; flags: qr rd ra; QUERY: 1, ANSWER: 1, AUTHORITY: 0, ADDITIONAL: 1

;; OPT PSEUDOSECTION:
; EDNS: version: 0, flags:; udp: 512
;; QUESTION SECTION:
;example.com.			IN	A

;; ANSWER SECTION:
example.com.		172800	IN	A	93.184.216.34

;; Query time: 50 msec
;; SERVER: 127.0.0.1#53(127.0.0.1)
;; WHEN: Wed Jul 27 10:00:00 UTC 2024
;; MSG SIZE  rcvd: 56
```

如果你能看到类似于上面的响应，**恭喜你！** Load Ants 已经成功运行，并将你的 DNS 查询通过 DoH 加密发送了出去。

---

### 下一步

-   [➡️ 使用 Docker 部署](./docker.md)
-   [➡️ 从源码构建](./build-from-source.md)
-   [➡️ 了解核心概念](../concepts/index.md)
