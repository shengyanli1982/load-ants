English | [中文](./README_CN.md)

<div align="center">
    <h1>LOAD ANTS</h1>
    <h4>🐜🐜🐜 Lightweight DNS Splitter and Forwarder: Seamless Conversion from UDP/TCP Queries to DoH, Supports DoH Proxy Forwarding</h4>
    <a href="https://shengyanli1982.github.io/load-ants/">
        <img src="./images/logo.png" alt="logo" width="600">
    </a>
</div>

<p align="center">
    <!-- 徽章将放在这里。可以添加构建状态、许可证等相关徽章。 -->
    <a href="https://github.com/shengyanli1982/load-ants/blob/main/LICENSE"><img src="https://img.shields.io/github/license/shengyanli1982/load-ants" alt="license"></a>
</p>

**Load Ants** 是一款专为提升网络隐私、安全与灵活性而生的高性能、多功能 DNS 代理服务。

### 核心功能

-   🔄 **协议转换**: 无缝将标准 DNS 查询转换为 DNS-over-HTTPS (DoH)。
-   🧠 **智能路由**: 根据域名模式（精确、通配符、正则）路由 DNS 查询。
-   ⚡ **高性能缓存**: 内置缓存机制，显著降低延迟。
-   🌐 **灵活上游管理**: 可将 DoH 服务器分组并配置多种负载均衡策略。

### 📚 阅读完整文档！

所有详细文档，包括配置、部署指南和高级用法，均已迁移至我们的专属文档网站。

**[➡️ 访问官方文档网站](https://shengyanli1982.github.io/load-ants/)**

### 🚀 快速开始

使用 Docker 在几秒钟内启动并运行：

```bash
# 为您的配置创建一个目录
mkdir -p ./load-ants-config
# 下载默认配置以开始使用
wget -O ./load-ants-config/config.yaml https://raw.githubusercontent.com/shengyanli1982/load-ants/main/config.default.yaml
# 根据您的需求编辑 config.yaml，然后运行：
docker run -d \
  --name load-ants \
  -p 53:53/udp \
  -p 53:53/tcp \
  -p 8080:8080 \
  -v $(pwd)/load-ants-config:/etc/load-ants \
  ghcr.io/shengyanli1982/load-ants-x64:latest -c /etc/load-ants/config.yaml
```

### 开源许可

本项目采用 [MIT 许可证](./LICENSE) 授权。
