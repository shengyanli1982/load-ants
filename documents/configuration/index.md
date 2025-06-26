# 总览

欢迎来到 Load Ants 的配置指南中心。本章节将带你深入了解 `config.yaml` 文件中的每一个配置项，帮助你打造出完全符合个人需求的 DNS 服务。

### 配置文件加载

Load Ants 在启动时会按照以下顺序寻找并加载配置文件：

1.  **通过 `-c` 或 `--config` 参数指定**:
    这是最推荐的方式，可以明确指定配置文件的路径。

    ```bash
    ./load-ants -c /path/to/your/config.yaml
    ```

2.  **当前工作目录**:
    如果在启动时没有使用 `-c` 参数，Load Ants 会尝试从其**当前工作目录**下加载名为 `config.yaml` 的文件。

3.  **系统默认目录 (Linux/macOS)**:
    如果以上路径均未找到，它会最后尝试从 `/etc/load-ants/config.yaml` 加载。

> ✨ **专家提示**:
> 为了保证可移植性和清晰性，强烈建议始终将 `config.yaml` 文件与 Load Ants 程序放在同一个目录下，并使用 `-c ./config.yaml` 的方式来显式加载它。

### YAML 语法基础

Load Ants 的配置文件使用 [YAML](https://yaml.org/) 格式。YAML 是一种对人类非常友好的数据序列化语言，其基本规则非常简单：

-   **使用缩进表示层级**: YAML 使用空格缩进（**不允许使用 Tab**）来表示数据的层级关系。通常建议使用 **2 个空格**作为一级缩进。
-   **键值对**: 使用冒号 (`:`) 分隔键和值，冒号后必须跟一个空格。
    ```yaml
    key: value
    ```
-   **列表 (数组)**: 使用短横线 (`-`) 加一个空格来表示列表中的一个元素。
    ```yaml
    list:
        - item1
        - item2
    ```
-   **注释**: 使用井号 (`#`) 来添加注释，从 `#` 开始到行尾的内容都会被忽略。

### 配置文件顶层结构

一个完整的 `config.yaml` 文件主要由以下几个顶层配置块组成。你可以根据自己的需求，选择性地启用或忽略某些非必选的配置块。

```yaml
# 服务器监听设置 (必选)
server:
    # ...

# 健康检查与管理服务器设置 (可选)
admin:
    # ...

# 缓存设置 (可选)
cache:
    # ...

# HTTP 客户端设置 (全局) (可选)
http_client:
    # ...

# 上游 DoH 服务器组 (必选)
upstream_groups:
    # ...

# 路由规则（静态配置） (可选)
static_rules:
    # ...

# 远程规则配置 (可选)
remote_rules:
    # ...
```

接下来，我们将分章节详细拆解每一个配置块。

-   [`server`](./server.md): 配置 DNS 服务的监听地址和参数。
-   [`admin`](./server.md#admin-管理服务器): 配置健康检查与管理 API 的监听地址。
-   [`cache`](./cache.md): 配置内置 DNS 缓存的行为。
-   [`http_client`](./http-client.md): 定义全局 HTTP 客户端的行为，影响所有出站请求。
-   [`upstream_groups`](./upstream-groups.md): 定义所有可用的上游 DoH 服务器组。
-   [`static_rules` & `remote_rules`](./routing-rules.md): 定义静态及远程加载的路由规则。
