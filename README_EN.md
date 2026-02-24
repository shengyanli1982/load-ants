[中文](./README.md) | English

<div align="center">
    <h1>LOAD ANTS</h1>
    <h4>🐜🐜🐜 Lightweight DNS Splitter and Forwarder: Seamless Conversion from UDP/TCP Queries to DoH, Supports DoH Proxy Forwarding</h4></br></br>
    <a href="https://shengyanli1982.github.io/load-ants/">
        <img src="./images/logo.png" alt="logo" width="600">
    </a>
</div>

<p align>
    <!-- Badges will go here. Add relevant badges for build status, license, etc. -->
    <a href="https://github.com/shengyanli1982/load-ants/blob/main/LICENSE"><img src="https://img.shields.io/github/license/shengyanli1982/load-ants" alt="license"></a>
</p>

**Load Ants** is a high-performance, multi-functional DNS proxy service designed to enhance your network privacy, security, and flexibility.

### Core Features

-   🔄 **Protocol Conversion**: Seamlessly converts standard DNS queries to DNS-over-HTTPS (DoH).
-   🧠 **Intelligent Routing**: Route DNS queries based on domain patterns (exact, wildcard, regex).
-   ⚡ **High-Performance Caching**: Built-in caching to reduce latency.
-   🌐 **Flexible Upstream Management**: Group DoH servers with multiple load-balancing strategies.

### 📚 Get the Full Picture!

All detailed documentation, including configuration, deployment guides, and advanced usage, has been moved to our dedicated documentation site.

**[➡️ Visit the Official Documentation Site](https://shengyanli1982.github.io/load-ants/)**

### Config Validation (Important)

- You must configure at least one rule source: `static_rules` and/or `remote_rules`.
- You must have at least one rule with `action: forward`, otherwise the program will fail fast during startup.
- If any `forward` rule exists: `upstream_groups` must be configured, and every `target` must reference an existing upstream group name.
- Validate your config and exit:

```bash
loadants --test -c ./config.yaml
```

### 🚀 Quick Start

Get up and running in seconds with Docker:

```bash
# Create a directory for your configuration
mkdir -p ./load-ants-config
# Download the default config to get started
wget -O ./load-ants-config/config.yaml https://raw.githubusercontent.com/shengyanli1982/load-ants/main/config.default.yaml
# Edit config.yaml to your needs, then run:
docker run -d \
  --name load-ants \
  -p 53:53/udp \
  -p 53:53/tcp \
  -p 8080:8080 \
  -v $(pwd)/load-ants-config:/etc/load-ants \
  ghcr.io/shengyanli1982/load-ants-x64:latest -c /etc/load-ants/config.yaml
```

### License

This project is licensed under the [MIT License](./LICENSE).
