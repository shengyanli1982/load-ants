# 从源码构建

对于希望深入了解项目、进行二次开发或在特定平台（官方未提供预编译版本）上运行 Load Ants 的开发者来说，从源码构建是一个很好的选择。

本指南将引导你完成从克隆仓库到生成可执行文件的完整过程。

---

### 环境要求

在开始之前，请确保你的系统满足以下条件：

-   **Git**: 用于克隆项目源代码。
-   **Rust 工具链**: 这是构建项目的核心。根据 `README` 文件，项目推荐使用：

    -   Rust `1.84.1` 或更高版本。
    -   GCC `14.2.0` 或更高版本。

    如果你尚未安装 Rust，我们强烈建议通过 [rustup](https://rustup.rs/) 官方安装脚本来安装和管理你的 Rust 版本。`rustup` 会自动处理好编译器、包管理器 (`cargo`) 和标准库。

    ```bash
    # 通过 rustup 安装 Rust
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    ```

### 步骤一：获取源代码

使用 `git` 从 GitHub 克隆 Load Ants 的官方仓库。

```bash
git clone https://github.com/eyas-ran/load-ants.git
cd load-ants
```

### 步骤二：构建项目

进入项目根目录后，使用 `cargo`（Rust 的包管理器和构建工具）来编译项目。

我们推荐构建**发布版本**（release build），这会应用大量优化，使最终生成的可执行文件性能更高。

```bash
cargo build --release
```

编译过程可能需要几分钟，`cargo` 会自动下载并编译所有依赖的库。

### 步骤三：运行可执行文件

构建成功后，你可以在 `target/release/` 目录下找到生成的可执行文件。

-   **在 Linux / macOS 上**: 文件名为 `load-ants`
-   **在 Windows 上**: 文件名为 `load-ants.exe`

现在，你可以像在 [快速上手](./index.md) 指南中一样运行它：

1.  **准备配置文件**:
    将项目根目录下的 `config.default.yaml` 复制一份，重命名为 `config.yaml`，并放置在你希望运行程序的任何位置。

2.  **运行程序**:

    ```bash
    # 在 Linux 或 macOS 上
    # 将编译好的文件和配置文件放在一起运行
    ./target/release/load-ants -c ./config.yaml

    # 在 Windows 上
    .\target\release\load-ants.exe -c .\config.yaml
    ```

    > **注意**：如果你的 `config.yaml` 中配置了需要特权的端口（如 53），你可能需要使用 `sudo` 或以管理员身份运行此命令。

恭喜你，现在你已经成功地从源代码构建并运行了 Load Ants！
