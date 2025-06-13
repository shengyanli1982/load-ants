English | [中文](./README_CN.md)

<div align="center">
    <h1>LOAD ANTS</h1>
    <h4>🐜🐜🐜 Lightweight DNS Forwarder: Seamless Conversion from UDP/TCP Queries to DoH</h4></br>
    <img src="./images/logo.png" alt="logo" width="600">
</div>

<p align="center">
  <a href="#introduction">Introduction</a> |
  <a href="#quick-start">Quick Start</a> |
  <a href="#core-features">Core Features</a> |
  <a href="#configuration-guide">Configuration Guide</a> |
  <a href="#installation-and-advanced-usage">Installation & Advanced Usage</a> |
  <a href="#in-depth-understanding">In-Depth Understanding</a>
</p>

## Introduction

**Load Ants** is a high-performance, multi-functional DNS proxy service that converts traditional UDP/TCP DNS queries to DNS-over-HTTPS (DoH). It serves as a bridge between clients using standard DNS protocols and modern secure DoH providers, offering enhanced privacy protection, security, and flexible routing capabilities.

### Why DNS-over-HTTPS?

Traditional DNS queries are transmitted in plaintext, making your browsing history vulnerable to monitoring, hijacking, or tampering. DoH addresses these issues through:

-   **DNS Traffic Encryption** - Prevents network man-in-the-middle snooping
-   **Enhanced Privacy** - Hides DNS query content from ISPs and other network observers
-   **Improved Security** - Effectively reduces the risk of DNS poisoning and spoofing attacks
-   **Circumvention of Network Restrictions** - Helps bypass DNS-based network blocking techniques

## Quick Start

This section will guide you through quickly deploying and running Load Ants. We recommend first trying to run the pre-compiled application directly, or using Docker if convenient.

### Requirements

-   **General**:
    -   A text editor for creating and modifying the configuration file (`config.yaml`).
    -   Administrator/root privileges (if Load Ants needs to bind to the standard DNS port 53).
-   **Running Pre-compiled Binary**:
    -   Binary file corresponding to your operating system, downloaded from the project's [release page](https://github.com/shengyanli1982/load-ants/releases).
-   **Using Docker**:
    -   Docker installed and running.
-   **(Optional) Building from Source (Advanced)**:
    -   Rust toolchain (Rust 1.84.1, GCC 14.2.0).

### Method 1: Running the Application Directly

This is the quickest way to experience Load Ants, especially if you want to run it directly on your machine.

1.  **Download Pre-compiled Version**:
    Visit the project's [release page](https://github.com/shengyanli1982/load-ants/releases), find and download the latest `load-ants` binary for your operating system.

2.  **Prepare Configuration File**:
    Download or copy the example configuration file `config.default.yaml` (usually provided with the source code or attached to the release). Place it in the same directory as the downloaded `load-ants` binary, and rename it to `config.yaml`.

    ```bash
    # Assuming config.default.yaml has been obtained and placed in the current directory
    cp config.default.yaml ./config.yaml
    ```

    Then, open `config.yaml` with your preferred text editor and make modifications. At minimum, you need to configure an upstream DoH server group (`upstream_groups`). You can refer to the [Configuration Guide](#configuration-guide) section later in this document.

3.  **Grant Execution Permission (Linux/macOS)**:
    If you're using a Linux (x86_64) system, you may need to add execution permission to the downloaded binary:

    > ![NOTE]
    > If you're using another system, please adjust accordingly.

    ```bash
    chmod +x ./loadants-linux-amd64
    ```

4.  **Run Load Ants**:
    Open a terminal, navigate to the directory containing the `loadants` and `config.yaml` files, then execute:

    ```bash
    ./loadants-linux-amd64 -c ./config.yaml
    ```

    If Load Ants is configured to listen on the standard DNS port (such as 53), you may need administrator privileges to start it (for example, using `sudo ./loadants -c ./config.yaml` on Linux/macOS, or running Command Prompt or PowerShell as administrator on Windows).
    Once started, the program will begin processing DNS requests according to the configuration file. Log information will be output directly to the terminal.

5.  **Basic Testing**:
    After the program is running, you can test DNS resolution using tools like `dig` (Linux/macOS) or `nslookup` (Windows):

    ```bash
    # Assuming Load Ants is listening on 127.0.0.1:53
    dig @127.0.0.1 example.com
    ```

    If everything is configured correctly, you should receive DNS responses from the upstream DoH server.

6.  **Stopping the Program**:
    Press `Ctrl+C` in the terminal running Load Ants to stop the program.

### Method 2: Deploying with Docker (Recommended)

Docker provides an isolated environment to run Load Ants without directly installing any dependencies on your system (except Docker itself).

1.  **Create Configuration Directory**:
    Create a directory on your host to store the configuration file.

    ```bash
    mkdir -p ./load-ants-config
    ```

2.  **Prepare Configuration File**:
    Copy the default configuration file (`config.default.yaml`) from the project to the directory you just created, edit it according to your needs, and name it `config.yaml`.

    ```bash
    # Assuming you've cloned the project or downloaded config.default.yaml
    cp config.default.yaml ./load-ants-config/config.yaml
    # Use your preferred editor to modify ./load-ants-config/config.yaml
    ```

    You'll need to configure at least the upstream DoH servers (`upstream_groups`).

3.  **Run Load Ants Container**:

    ```bash
    docker run -d \
      --name load-ants \
      -p 53:53/udp \
      -p 53:53/tcp \
      -p 8080:8080 \
      -v $(pwd)/load-ants-config:/etc/load-ants \
      ghcr.io/shengyanli1982/load-ants-x64:latest -c /etc/load-ants/config.yaml
    ```

    Please replace `ghcr.io/shengyanli1982/load-ants-x64:latest` with your own built image name or the latest official image.
    Command explanation:

    -   `-d`: Run container in the background.
    -   `--name load-ants`: Name the container.
    -   `-p 53:53/udp -p 53:53/tcp`: Map the host's DNS ports to the container.
    -   `-p 8080:8080`: Map the management port (health check, metrics).
    -   `-v $(pwd)/load-ants-config:/etc/load-ants`: Mount the host's configuration directory into the container. Make sure `$(pwd)/load-ants-config` resolves to an absolute path, or provide an absolute path directly.
    -   `ghcr.io/shengyanli1982/load-ants-x64:latest`: Docker image to use.
    -   `-c /etc/load-ants/config.yaml`: Specify the path to the configuration file inside the container.

4.  **Basic Testing**:
    After the container starts, you can test the service as follows:

    -   **Test DNS Resolution**:
        Use `dig` or other DNS query tools to send query requests to `127.0.0.1` (or your server IP).
        ```bash
        dig @127.0.0.1 example.com
        ```
        If configured correctly, you should receive responses from the upstream DoH server.
    -   **View Logs**:
        Check Load Ants' running logs to understand its working status or troubleshoot issues.
        ```bash
        docker logs load-ants
        ```

5.  **Stop and Remove Container** (if needed):
    ```bash
    docker stop load-ants
    docker rm load-ants
    ```

## Core Features

-   🔄 **Protocol Conversion**
    -   Seamlessly converts UDP/53 and TCP/53 DNS requests to DoH (RFC 8484)
    -   Full support for GET and POST HTTP methods
    -   Handles multiple content formats, including `application/dns-message` and `application/dns-json`
-   🧠 **Intelligent Routing**
    -   **Flexible Matching** - Precisely route DNS queries based on domain patterns:
        -   Exact domain matching
        -   Wildcard domain matching (like `*.example.com`)
        -   Regular expression domain matching
    -   **Custom Actions** - Define precise handling for each match:
        -   Forward to specific upstream DoH groups
        -   Block queries (return NXDOMAIN)
    -   **Remote Rule Lists** - Support for loading and merging external rule lists from URLs (e.g., V2Ray format `reject-list.txt`, `proxy-list.txt`)
-   🌐 **Flexible Upstream Management**
    -   **Grouping Mechanism** - Organize DoH servers into independently configured logical groups
    -   **Load Balancing** - Configure efficient balancing strategies for each group:
        -   Round Robin (RR) - Evenly distribute requests among servers
        -   Weighted Round Robin (WRR) - Prioritize servers based on weight
        -   Random Distribution - Non-deterministic selection for enhanced privacy
    -   **Authentication Support** - Securely communicate with private DoH providers requiring authentication:
        -   HTTP Basic Authentication
        -   Bearer Token Authentication
    -   **Resource Optimization** - All upstream groups share HTTP client connection pools for improved resource utilization
-   ⚡ **Performance Optimization**
    -   **Intelligent Caching** - Built-in DNS caching mechanism significantly reduces latency and upstream load
        -   **Positive Caching** - Store successful DNS responses to speed up resolution process
        -   **Negative Caching** - Cache error responses (NXDOMAIN, ServFail, etc.) to avoid repeated queries for non-existent domains
        -   **Adjustable TTL** - Set differentiated time-to-live for positive and negative cache entries
    -   **Connection Pool Reuse** - Efficiently reuse HTTP connections for better performance
    -   **TTL Optimization** - Flexible configuration of minimum and maximum TTL values for cached responses
-   🔁 **High Reliability**
    -   **Smart Retry** - Automatically retry failed DoH requests with configurable attempt counts and delays
    -   **Timeout Control** - Precisely adjust connection and request timeout parameters
-   ⚙️ **Management Capabilities**
    -   **YAML Configuration** - Simple, readable configuration approach
    -   **Configuration Validation** - Strict configuration validation at startup or in test mode
    -   **Health Checks** - Complete monitoring integration interfaces for operations teams
    -   **Prometheus Metrics** - Comprehensive monitoring metrics via the `/metrics` endpoint

## Configuration Guide

Load Ants uses YAML format configuration files. Below is a complete reference of configuration options. We recommend starting by modifying `config.default.yaml`.

### Server Configuration (server)

| Parameter   | Type    | Default        | Description                           | Valid Range          |
| ----------- | ------- | -------------- | ------------------------------------- | -------------------- |
| listen_udp  | String  | "127.0.0.1:53" | UDP DNS listen address and port       | Valid IP:port format |
| listen_tcp  | String  | "127.0.0.1:53" | TCP DNS listen address and port       | Valid IP:port format |
| tcp_timeout | Integer | 10             | TCP connection idle timeout (seconds) | 1-3600               |

### Health Check Configuration (health)

This section configures the HTTP service that exposes health checks and monitoring metrics.

| Parameter | Type   | Default          | Description                                  | Valid Range          |
| --------- | ------ | ---------------- | -------------------------------------------- | -------------------- |
| listen    | String | "127.0.0.1:8080" | Health check service listen address and port | Valid IP:port format |

### Cache Configuration (cache)

Cache configuration allows fine-tuning of DNS response caching behavior.

| Parameter    | Type    | Default | Description                                                                  | Valid Range |
| ------------ | ------- | ------- | ---------------------------------------------------------------------------- | ----------- |
| enabled      | Boolean | true    | Whether to enable caching                                                    | true/false  |
| max_size     | Integer | 10000   | Maximum number of cache entries                                              | 10-1000000  |
| min_ttl      | Integer | 60      | Minimum TTL (seconds), overrides smaller TTLs in original responses          | 1-86400     |
| max_ttl      | Integer | 3600    | Maximum time-to-live upper limit for all cache entries (seconds)             | 1-86400     |
| negative_ttl | Integer | 300     | Negative cache TTL (seconds), for caching errors, non-existent domains, etc. | 1-86400     |

**About Negative Caching**:
Negative caching is an important performance optimization technique that caches DNS error responses (such as NXDOMAIN or ServFail) for a specified time. This effectively prevents repeated queries to upstream servers for non-existent or temporarily unresolvable domains, reducing latency and upstream server load.

### HTTP Client Configuration (http_client)

This configuration applies to all HTTP requests sent to upstream DoH servers.

| Parameter       | Type    | Default | Description                                  | Valid Range      |
| --------------- | ------- | ------- | -------------------------------------------- | ---------------- |
| connect_timeout | Integer | 5       | Connection timeout (seconds)                 | 1-120            |
| request_timeout | Integer | 10      | Request timeout (seconds)                    | 1-1200           |
| idle_timeout    | Integer | 60      | Idle connection timeout (seconds) (optional) | 5-1800           |
| keepalive       | Integer | 60      | TCP Keepalive (seconds) (optional)           | 5-600            |
| agent           | String  | -       | HTTP User Agent (optional)                   | Non-empty string |

### Upstream DoH Server Group Configuration (upstream_groups)

You can define one or more upstream DoH server groups, each containing multiple servers and having independent load balancing strategies, retry mechanisms, and proxy settings.

| Parameter | Type   | Default | Description                                                  | Valid Range                        |
| --------- | ------ | ------- | ------------------------------------------------------------ | ---------------------------------- |
| name      | String | -       | Group name (required, must be unique)                        | Non-empty string                   |
| strategy  | String | -       | Load balancing strategy (required)                           | "roundrobin", "weighted", "random" |
| servers   | Array  | -       | List of DoH servers in this group (required, at least one)   | -                                  |
| retry     | Object | -       | Request retry configuration for this group (optional)        | See retry configuration below      |
| proxy     | String | -       | Proxy to use when accessing servers in this group (optional) | Valid HTTP/SOCKS5 proxy URL        |

#### Server Configuration (servers)

Each element in the `servers` array of `upstream_groups` represents a DoH server.

| Parameter    | Type    | Default   | Description                                                            | Valid Range                  |
| ------------ | ------- | --------- | ---------------------------------------------------------------------- | ---------------------------- |
| url          | String  | -         | DoH server URL (required)                                              | Valid HTTP(S) URL with path  |
| weight       | Integer | 1         | Weight (only effective when group strategy is `weighted`)              | 1-65535                      |
| method       | String  | "post"    | DoH request method (GET or POST)                                       | "get", "post"                |
| content_type | String  | "message" | DoH content type (`application/dns-message` or `application/dns-json`) | "message", "json"            |
| auth         | Object  | -         | Authentication configuration for accessing this server (optional)      | See auth configuration below |

**Technical Considerations for DoH Content Types:**

-   `message` (`application/dns-message`): Implements the RFC 8484 standard protocol, supporting both GET and POST HTTP methods. This format encapsulates binary DNS messages directly and is the recommended option for optimal compatibility and performance across DoH providers.

-   `json` (`application/dns-json`): Implements Google's JSON API specification for DNS queries, which **exclusively supports the GET method**. This format is provided primarily for compatibility with specific client implementations that require JSON-formatted responses.

When configuring with `content_type: "json"`, you **must** specify `method: "get"`. The system's configuration validator enforces this protocol requirement and will reject any configuration that attempts to combine `content_type: "json"` with `method: "post"`, as this combination violates the Google Public DNS specification and would result in failed queries.

#### Authentication Configuration (auth)

Used for `upstream_groups.servers.auth`.

| Parameter | Type   | Default | Description                                | Valid Range       |
| --------- | ------ | ------- | ------------------------------------------ | ----------------- |
| type      | String | -       | Authentication type (required)             | "basic", "bearer" |
| username  | String | -       | Username (only for `basic` authentication) | Non-empty string  |
| password  | String | -       | Password (only for `basic` authentication) | Non-empty string  |
| token     | String | -       | Token (only for `bearer` authentication)   | Non-empty string  |

#### Retry Configuration (retry)

Used for `upstream_groups.retry`.

| Parameter | Type    | Default | Description              | Valid Range |
| --------- | ------- | ------- | ------------------------ | ----------- |
| attempts  | Integer | 3       | Number of retry attempts | 1-100       |
| delay     | Integer | 1       | Initial delay (seconds)  | 1-120       |

### Routing Rules Configuration (static_rules)

Define local static routing rules. Load Ants uses a priority-based matching system for DNS routing decisions:

1.  **Exact Matching** (`exact`) - Completely matches full domain names (e.g., `example.com`). Highest priority.
2.  **Specific Wildcard Matching** (`wildcard`) - Uses wildcards to match specific domain patterns (e.g., `*.example.com`).
3.  **Regular Expression Matching** (`regex`) - Uses regular expressions for complex matching (e.g., `^(mail|audio)\\.google\\.com$`).
4.  **Global Wildcard Matching** (`wildcard` pattern `*`) - Matches any domain. Lowest priority.

Typically, the global wildcard (`*`) should be used as the last rule, serving as the default option when no other rules match.

| Parameter | Type   | Default | Description                                                 | Valid Range                      |
| --------- | ------ | ------- | ----------------------------------------------------------- | -------------------------------- |
| match     | String | -       | Match type (required)                                       | "exact", "wildcard", "regex"     |
| patterns  | Array  | -       | List of match patterns (required, at least one pattern)     | Non-empty string array           |
| action    | String | -       | Routing action (required)                                   | "forward", "block"               |
| target    | String | -       | Target upstream group (required when `action` is `forward`) | Name of a defined upstream group |

### Remote Rules Configuration (remote_rules)

`remote_rules` allows the system to fetch domain rule lists from external URLs (such as block lists, proxy lists, etc.) and merge them with local static rules. These rules will be integrated into the routing engine according to their `action` (block or forward) and match type (exact, wildcard, regex) parsed from the remote file, following the same priority logic as static rules.

| Parameter | Type    | Default | Description                                                           | Valid Range                                         |
| --------- | ------- | ------- | --------------------------------------------------------------------- | --------------------------------------------------- |
| type      | String  | "url"   | Rule type, currently only "url" is supported                          | "url"                                               |
| url       | String  | -       | URL of the remote rule file (required)                                | Valid HTTP(S) URL                                   |
| format    | String  | "v2ray" | Rule file format                                                      | "v2ray" (may support "clash" etc. in the future)    |
| action    | String  | -       | Action to apply to all domains in this rule list (required)           | "block", "forward"                                  |
| target    | String  | -       | Target upstream group (required when `action` is `forward`)           | Name of a defined upstream group                    |
| retry     | Object  | -       | Retry strategy for fetching rules (optional)                          | See retry configuration within `remote_rules` below |
| proxy     | String  | -       | HTTP/SOCKS5 proxy to use when fetching rules (optional)               | Valid proxy URL                                     |
| auth      | Object  | -       | Authentication configuration for accessing remote rule URL (optional) | See auth configuration within `remote_rules` below  |
| max_size  | Integer | 1048576 | Maximum size of remote rule file (bytes), e.g., 1048576 means 1MB     | 1 - N (e.g., 10485760 for 10MB)                     |

#### Retry Configuration (retry) - within remote_rules

Used for `remote_rules.retry`.

| Parameter | Type    | Default | Description              | Valid Range |
| --------- | ------- | ------- | ------------------------ | ----------- |
| attempts  | Integer | 3       | Number of retry attempts | 1-100       |
| delay     | Integer | 1       | Initial delay (seconds)  | 1-120       |

#### Authentication Configuration (auth) - within remote_rules

Used for `remote_rules.auth`, structure is the same as `upstream_groups.servers.auth`.

| Parameter | Type   | Default | Description                                | Valid Range       |
| --------- | ------ | ------- | ------------------------------------------ | ----------------- |
| type      | String | -       | Authentication type (required)             | "basic", "bearer" |
| username  | String | -       | Username (only for `basic` authentication) | Non-empty string  |
| password  | String | -       | Password (only for `basic` authentication) | Non-empty string  |
| token     | String | -       | Token (only for `bearer` authentication)   | Non-empty string  |

**`remote_rules` Example:**

```yaml
remote_rules:
    # Fetch block list from URL, using Bearer authentication and proxy
    - type: "url"
      url: "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/reject-list.txt"
      format: "v2ray"
      action: "block"
      retry:
          attempts: 3
          delay: 1
      proxy: "http://user:pass@proxyserver:port"
      auth:
          type: "bearer"
          token: "your_secure_token"
      max_size: 1048576 # 1MB

    # Fetch forward list from URL, specifying target upstream group
    - type: "url"
      url: "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/proxy-list.txt"
      format: "v2ray"
      action: "forward"
      target: "google_public" # Forward to the upstream group named "google_public"
```

### Configuration Example

This is a complete example including most common configurations:

```yaml
# Load Ants Configuration Example

# Server listening settings
server:
    listen_udp: "0.0.0.0:53" # UDP listen address and port
    listen_tcp: "0.0.0.0:53" # TCP listen address and port
    tcp_timeout: 10 # TCP connection idle timeout (seconds)

# Health check and management server settings
health:
    listen: "0.0.0.0:8080" # Health check server listen address and port

# Cache settings
cache:
    enabled: true
    max_size: 10000 # Maximum number of cache entries
    min_ttl: 60 # Minimum TTL for cache entries (seconds)
    max_ttl: 3600 # Maximum TTL for cache entries (seconds)
    negative_ttl: 300 # Negative cache TTL (seconds), for caching error responses

# HTTP client settings (global)
http_client:
    connect_timeout: 5 # Connection timeout (seconds)
    request_timeout: 10 # Request timeout (seconds)
    idle_timeout: 60 # Idle connection timeout (seconds) (optional)
    keepalive: 60 # TCP Keepalive (seconds) (optional)
    agent: "LoadAnts/1.0" # Custom User-Agent (optional)

# Upstream DoH server groups
upstream_groups:
    - name: "google_public"
      strategy: "roundrobin" # Options: roundrobin, weighted, random
      servers:
          - url: "https://dns.google/dns-query"
            method: "post" # Options: get, post (default: post)
            content_type: "message" # Options: message, json (default: message)
          - url: "https://8.8.4.4/dns-query"
            method: "get"
      retry: # Retry strategy for this group (optional)
          attempts: 3
          delay: 1 # seconds
      proxy: "http://user:pass@proxyserver:port" # Proxy for this group (optional)

    - name: "cloudflare_secure"
      strategy: "random"
      servers:
          - url: "https://cloudflare-dns.com/dns-query"
          - url: "https://1.0.0.1/dns-query"
            content_type: "json"

    - name: "nextdns_authenticated"
      strategy: "weighted"
      servers:
          - url: "https://dns.nextdns.io/YOUR_CONFIG_ID_1"
            weight: 70
            auth: # Server-specific authentication (optional)
                type: "bearer" # basic or bearer
                token: "YOUR_API_KEY_OR_TOKEN_1"
          - url: "https://dns.nextdns.io/YOUR_CONFIG_ID_2" # Note: NextDNS typically uses the same config ID
            weight: 30
            auth:
                type: "basic"
                username: "your_username"
                password: "your_password"
      # retry: # This group can have its own retry strategy (optional)
      # proxy: # This group can also have its own proxy (optional)

# Routing rules (static configuration)
static_rules:
    # Block specific domains
    - match: "exact"
      patterns: ["ads.example.com", "tracker.example.org"]
      action: "block"

    # Route internal domains to a specific upstream group
    - match: "wildcard"
      patterns: ["*.corp.internal", "*.mycompany.local"]
      action: "forward"
      target: "cloudflare_secure" # Reference to upstream group name defined above

    # Use regular expressions to match and forward
    - match: "regex"
      patterns: ["^(.*\.)?google\.com$", "^(.*\.)?gstatic\.com$"] # Match google.com and its subdomains
      action: "forward"
      target: "google_public"

    # Default rule: forward all other traffic to google_public (ensure this is the last rule)
    - match: "wildcard"
      patterns: ["*"] # Match all domains
      action: "forward"
      target: "google_public"

# Remote rules configuration (load rules from URL)
remote_rules:
  # Example: Load V2Ray format block list from URL
  - type: "url"
    url: "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/reject-list.txt"
    format: "v2ray" # Currently supports v2ray
    action: "block" # Domains in the rule list will be blocked
    retry: # Retry strategy for fetching this rule list (optional)
      attempts: 3
      delay: 2
    proxy: "socks5://localhost:1080" # Proxy to use when fetching this rule list (optional)
    auth: # Authentication for fetching this rule list (optional)
      type: "bearer"
      token: "some_token"
    max_size: 2097152 # Maximum size for this rule file (e.g. 2MB)

  # Example: Load V2Ray format proxy (forward) list from URL
  - type: "url"
    url: "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/direct-list.txt" # Typically direct-list is used for direct connection, here we assume it's used for forwarding
    format: "v2ray"
    action: "forward"
    target: "cloudflare_secure" # Domains in the rule list will be forwarded to the cloudflare_secure group
```

## Installation and Advanced Usage

### Command Line Arguments

```text
load-ants [OPTIONS]

Options:
    -c, --config <PATH>    Specify configuration file path
                           (Default search order: ./config.yaml, /etc/load-ants/config.yaml)
    -t, --test             Test configuration file validity and exit
    -h, --help             Display help information
    -V, --version          Display version information
```

### Building from Source

If you want to build or modify the code yourself:

1.  **Environment Preparation**:

    -   Install the [Rust toolchain](https://www.rust-lang.org/tools/install) (Rust 1.84.1, GCC 14.2.0).
    -   Ensure your system has `git` installed.

2.  **Clone the Repository**:

    ```bash
    git clone https://github.com/shengyanli1982/load-ants.git
    cd load-ants
    ```

    (Please replace `shengyanli1982/load-ants.git` with the actual repository address)

3.  **Build the Application**:

    ```bash
    cargo build --release
    ```

    The compiled binary will be located at `target/release/load-ants`.

4.  **Run**:
    You'll need a configuration file (for example, copied and modified from `config.default.yaml`).
    ```bash
    # Assuming the configuration file is at ./config.yaml
    sudo ./target/release/load-ants -c ./config.yaml
    ```
    If you need to listen on the standard DNS port (53), you typically need `sudo` privileges.

Compiled binaries can also be downloaded directly from the project's [release page](https://github.com/shengyanli1982/load-ants/releases) (if provided).

### Running as a System Service (Linux systemd)

If you want to run Load Ants as a background service on a Linux system:

1.  **Prepare Binary and Configuration Files**:

    -   Place the compiled `load-ants` binary in an appropriate system path, such as `/usr/local/bin/load-ants`.
    -   Place your configuration file in a location like `/etc/load-ants/config.yaml`.
        ```bash
        sudo mkdir -p /etc/load-ants
        sudo cp /path/to/your/config.yaml /etc/load-ants/config.yaml
        sudo cp ./target/release/load-ants /usr/local/bin/
        sudo chmod +x /usr/local/bin/load-ants
        ```

2.  **Create Service File**:
    Create the file `/etc/systemd/system/load-ants.service` with the following content:

    ```ini
    [Unit]
    Description=Load Ants DNS to DoH Proxy Service
    After=network.target network-online.target
    Requires=network-online.target

    [Service]
    Type=simple
    ExecStart=/usr/local/bin/load-ants -c /etc/load-ants/config.yaml
    Restart=on-failure
    RestartSec=5s
    User=root # Or another user with permission to bind to port 53; for non-root users, you may need to set CAP_NET_BIND_SERVICE
    Group=root # Or the corresponding user group
    # AmbientCapabilities=CAP_NET_BIND_SERVICE # If running as a non-root user and binding to privileged ports

    # Security enhancements (optional)
    ProtectSystem=full
    ProtectHome=true
    PrivateTmp=true
    NoNewPrivileges=true

    [Install]
    WantedBy=multi-user.target
    ```

3.  **Reload systemd Configuration and Start the Service**:

    ```bash
    sudo systemctl daemon-reload
    sudo systemctl enable load-ants.service
    sudo systemctl start load-ants.service
    ```

4.  **Check Service Status**:
    ```bash
    sudo systemctl status load-ants.service
    journalctl -u load-ants.service -f # View real-time logs
    ```

### Kubernetes Deployment

For production environments, Kubernetes provides better scalability, high availability, and management convenience.

1.  **Create Docker Image (if not yet published)**:
    If your project includes a `Dockerfile`, you'll need to build and push a Docker image to a registry (such as Docker Hub, GCR, ECR).

    ```bash
    # Assuming Dockerfile is in the project root directory
    docker build -t yourusername/load-ants:latest .
    docker push yourusername/load-ants:latest
    ```

    (Please replace `yourusername/load-ants:latest` with your actual image name and tag)

2.  **Create ConfigMap**:
    Store your `config.yaml` content in a Kubernetes ConfigMap.

    ```yaml
    # load-ants-configmap.yaml
    apiVersion: v1
    kind: ConfigMap
    metadata:
        name: load-ants-config
        namespace: dns # Recommended to use a separate namespace for DNS-related services
    data:
        config.yaml: |
            # Paste your complete config.yaml content here
            server:
              listen_udp: "0.0.0.0:53"
              listen_tcp: "0.0.0.0:53"
            health:
              listen: "0.0.0.0:8080"
            cache:
              enabled: true
              max_size: 10000
              # ... other configurations ...
            upstream_groups:
              - name: "google_public"
                strategy: "roundrobin"
                servers:
                  - url: "https://dns.google/dns-query"
                # ... more upstream configurations ...
            # ... more rule configurations ...
    ```

3.  **Create Deployment**:
    Define a Deployment to manage Load Ants Pods.

    ```yaml
    # load-ants-deployment.yaml
    apiVersion: apps/v1
    kind: Deployment
    metadata:
        name: load-ants
        namespace: dns
        labels:
            app: load-ants
    spec:
        replicas: 2 # Adjust replica count according to your needs
        selector:
            matchLabels:
                app: load-ants
        template:
            metadata:
                labels:
                    app: load-ants
            spec:
                containers:
                    - name: load-ants
                      image: ghcr.io/shengyanli1982/load-ants-x64:latest # Use official or your own built image
                      args: ["-c", "/etc/load-ants/config.yaml"]
                      ports:
                          - containerPort: 53
                            name: dns-udp
                            protocol: UDP
                          - containerPort: 53
                            name: dns-tcp
                            protocol: TCP
                          - containerPort: 8080
                            name: http-admin # For health checks and metrics
                      volumeMounts:
                          - name: config-volume
                            mountPath: /etc/load-ants # Mount configuration file directory
                      resources: # Adjust resource requests and limits based on actual situations
                          limits:
                              memory: "256Mi"
                              cpu: "500m"
                          requests:
                              memory: "128Mi"
                              cpu: "100m"
                      livenessProbe: # Health check
                          httpGet:
                              path: /health
                              port: http-admin
                          initialDelaySeconds: 15
                          periodSeconds: 20
                      readinessProbe: # Readiness probe
                          httpGet:
                              path: /health
                              port: http-admin
                          initialDelaySeconds: 5
                          periodSeconds: 10
                volumes:
                    - name: config-volume
                      configMap:
                          name: load-ants-config # Reference to the ConfigMap created above
    ```

4.  **Create Service**:
    Expose the Load Ants service for access within or outside the cluster.

    ```yaml
    # load-ants-service.yaml
    apiVersion: v1
    kind: Service
    metadata:
        name: load-ants-svc
        namespace: dns
    spec:
        selector:
            app: load-ants # Match the Pod label in the Deployment
        ports:
            - name: dns-udp
              port: 53
              protocol: UDP
              targetPort: dns-udp
            - name: dns-tcp
              port: 53
              protocol: TCP
              targetPort: dns-tcp
            - name: http-admin
              port: 8080
              protocol: TCP
              targetPort: http-admin
        # type: ClusterIP # Default, accessible only within the cluster. Other Pods in the cluster can access via load-ants-svc.dns:53.
        type: LoadBalancer # If external access is needed and supported by cloud provider (will assign a public IP).
        # type: NodePort # If you need to expose the service on a specific port on each node.
    ```

5.  **Apply Configuration to the Cluster**:

    ```bash
    kubectl create namespace dns # If not already created
    kubectl apply -f load-ants-configmap.yaml
    kubectl apply -f load-ants-deployment.yaml
    kubectl apply -f load-ants-service.yaml
    ```

6.  **Check Deployment Status**:
    ```bash
    kubectl -n dns get pods -l app=load-ants
    kubectl -n dns get svc load-ants-svc
    kubectl -n dns logs -l app=load-ants -f # View Pod real-time logs
    ```

## In-Depth Understanding

### Architecture Design

Load Ants adopts a modular architecture design, including the following core components:

-   **Server Module (`server`)**: Listens on UDP/53 and TCP/53 ports, receiving traditional DNS queries.
-   **DNS Parser (`parser` & `composer`)**: Parses incoming DNS requests and constructs DNS responses.
-   **Processor Module (`processor`)**: Coordinates the query processing flow, including cache checking, routing decisions, and upstream forwarding.
-   **Cache Module (`cache`)**: Implements efficient DNS caching (both positive and negative), reducing latency and upstream load.
-   **Router Module (`router`)**: Matches domains based on configured rules (static and remote) and determines query actions (forward or block) and targets.
-   **Upstream Management Module (`upstream`)**: Manages DoH upstream server groups, handles HTTP(S) communication with DoH servers, and implements load balancing, authentication, and retry logic.
-   **HTTP Client (`http_client`)**: Globally shared HTTP client for communication with upstream DoH servers and remote rule URLs.
-   **Remote Rule Loader (`remote_rule`)**: Responsible for fetching, parsing, and converting remote rule lists from URLs, with support for retries, proxies, and authentication.
-   **Administration and Monitoring Module (`admin`/`health`/`metrics`)**: Provides HTTP endpoints for health checks (`/health`), Prometheus metrics (`/metrics`), and cache refreshing (`/api/cache/refresh`).

![architecture](./images/architecture.png)
_Figure: Load Ants Architecture Diagram_

### Prometheus Monitoring Metrics

Load Ants provides comprehensive Prometheus monitoring metrics for real-time monitoring of service performance, health status, and operational conditions. These metrics are exposed through the `/metrics` endpoint (default listening on `0.0.0.0:8080/metrics`, configurable via `health.listen`), and can be collected by Prometheus or other compatible monitoring systems.

![metrics](./images/metrics.png)
_Figure: Load Ants Prometheus Metrics Example (Grafana Dashboard)_

#### Main Metric Categories:

-   **DNS Request Metrics**:
    -   `loadants_dns_requests_total`: Total number of DNS requests processed (labels: `protocol` (UDP/TCP)).
    -   `loadants_dns_request_duration_seconds`: DNS request processing time histogram (labels: `protocol`, `query_type`).
    -   `loadants_dns_request_errors_total`: Total number of DNS request processing errors (labels: `error_type`).
-   **Cache Metrics**:
    -   `loadants_cache_entries`: Current number of DNS cache entries.
    -   `loadants_cache_capacity`: Maximum capacity of the DNS cache.
    -   `loadants_cache_operations_total`: Total number of cache operations (labels: `operation` (hit, miss, insert, evict, expire)).
    -   `loadants_cache_ttl_seconds`: TTL distribution histogram of DNS cache entries (labels: `source`).
-   **DNS Query Details**:
    -   `loadants_dns_query_type_total`: Total number of DNS queries by record type (A, AAAA, MX, etc.) (labels: `type`).
    -   `loadants_dns_response_codes_total`: Total number of DNS responses by response code (RCODE) (labels: `rcode`).
-   **Upstream Resolver Metrics**:
    -   `loadants_upstream_requests_total`: Total number of requests sent to upstream DoH resolvers (labels: `group`, `server`).
    -   `loadants_upstream_errors_total`: Total number of upstream DoH resolver errors (labels: `error_type`, `group`, `server`).
    -   `loadants_upstream_duration_seconds`: Upstream DoH query time histogram (labels: `group`, `server`).
-   **Routing Metrics**:
    -   `loadants_route_matches_total`: Total number of routing rule matches (labels: `rule_type` (exact, wildcard, regex), `target_group`, `rule_source` (static, remote), `action` (block, forward)).
    -   `loadants_route_rules_count`: Current number of active routing rules (labels: `rule_type`, `rule_source`).

These rich metrics support detailed monitoring and analysis of Load Ants performance and behavior, helping to quickly identify issues, optimize configurations, and ensure the service meets performance requirements.

### API Endpoints

Load Ants provides the following HTTP API endpoints:

#### DNS Service Endpoints

-   **UDP and TCP port 53** (or other ports configured via `server.listen_udp` and `server.listen_tcp`)
    -   _Description_: Standard DNS ports for receiving traditional DNS queries.
    -   _Protocol_: DNS over UDP/TCP (RFC 1035).
    -   _Usage_: Standard DNS clients, applications, and systems send queries through these ports.

#### Management Endpoints

Default listening on `0.0.0.0:8080` (configurable via `health.listen`).

-   **GET /health**

    -   _Description_: Health check endpoint for service monitoring and Kubernetes liveness/readiness probes.
    -   _Returns_: `200 OK` with a simple JSON response `{"status":"healthy"}` when the service is healthy.
    -   _Usage_: `curl http://localhost:8080/health`

-   **GET /metrics**

    -   _Description_: Prometheus metrics endpoint exposing performance and operational statistics.
    -   _Content Type_: `text/plain; version=0.0.4; charset=utf-8`
    -   _Usage_: `curl http://localhost:8080/metrics`

-   **POST /api/cache/refresh**
    -   _Description_: Administrative endpoint for clearing the DNS cache.
    -   _Returns_: JSON response indicating success or error.
        -   Success: `200 OK` with `{"status":"success", "message":"DNS cache has been cleared"}`
        -   Cache not enabled: `400 Bad Request` with `{"status":"error", "message":"Cache is not enabled"}`
        -   Other errors: `500 Internal Server Error` with `{"status":"error", "message":"Failed to clear cache"}`
    -   _Usage_: `curl -X POST http://localhost:8080/api/cache/refresh`

API endpoints follow standard HTTP status codes.

### Use Cases

Load Ants is particularly well-suited for the following use cases:

-   **Individual Users/Home Networks**:
    -   Enhanced Privacy: Encrypt all DNS queries through DoH, preventing ISP or network man-in-the-middle snooping.
    -   Circumvent Blocking and Censorship: Bypass DNS-based network access restrictions by selecting appropriate DoH servers.
    -   Ad and Tracker Blocking: Effectively block advertisement domains and trackers by combining static rules and remote block lists (e.g., from `oisd.nl` or other sources).
    -   Custom Resolution: Specify specific upstream resolvers for specific domains (e.g., use specific DNS for specific services).
-   **Developers/Testing Environments**:
    -   Local DoH Resolution: Conveniently test applications that require DoH support locally.
    -   DNS Behavior Analysis: Observe application DNS query behavior through logs and metrics.
    -   Flexible Routing Testing: Quickly set up and test complex DNS routing policies, including dynamic updates based on remote lists.
-   **Enterprise/Organizational Internal Networks**:
    -   Centralized DNS Resolution: Unify management of internal network DNS queries, enforce encryption, and improve network security baseline.
    -   Security Policy Implementation: Block malicious domains, phishing sites, C&C servers, etc., with the ability to integrate threat intelligence sources.
    -   Internal Domain Resolution: Route internal domain resolution requests to internal DNS servers (if internal DNS supports DoH, or through another non-DoH proxy layer).
    -   Compliance: Log and audit DNS queries (requires self-configuration of log collection and analysis systems; Load Ants provides structured log output).
-   **Cloud-Native Environments (Kubernetes, Docker Swarm)**:
    -   Sidecar Proxy: Serve as a sidecar container providing DoH resolution capabilities for other applications in the cluster without modifying the applications themselves.
    -   Cluster DNS Service: Act as a cluster-wide DNS resolver (typically combined with or as an upstream for CoreDNS, etc., to enhance specific functionality).
    -   High-Performance DNS Gateway: Provide high-concurrency, low-latency DNS-to-DoH conversion and intelligent routing for large-scale applications.

## License

[MIT License](./LICENSE)

## Acknowledgements

-   Thanks to all developers who have contributed to the Load Ants project.
-   The design and implementation of this project were inspired by many excellent open-source DNS tools and DoH practices.
-   Special thanks to the Rust community for providing powerful tools and ecosystem.
