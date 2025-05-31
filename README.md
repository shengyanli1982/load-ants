English | [中文](./README_CN.md)

<div align="center">
    <h1>LOAD ANTS</h1>
    <h4>🐜🐜🐜 High-Performance DNS Forwarder: Seamlessly Converting UDP/TCP Queries to DNS-over-HTTPS</h4></br>
    <img src="./images/logo.png" alt="logo" width="600">
</div>

<p align="center">
  <a href="#introduction">Introduction</a>
  |
  <a href="#core-features">Core Features</a>
  |
  <a href="#architecture">Architecture</a>
  |
  <a href="#prometheus-metrics">Prometheus Metrics</a>
  |
  <a href="#api-endpoints">API Endpoints</a>
  |
  <a href="#use-cases">Use Cases</a>
  |
  <a href="#configuration-guide">Configuration</a>
  |
  <a href="#installation">Installation</a>
  |
  <a href="#usage">Usage</a>
</p>

## Introduction

**Load Ants** is a high-performance, enterprise-grade DNS proxy service that transforms traditional UDP/TCP DNS queries into secure DNS-over-HTTPS (DoH) requests. It functions as a seamless bridge between standard DNS clients and modern DoH providers, delivering enhanced privacy protection, robust security, and sophisticated routing capabilities.

### Why DNS-over-HTTPS?

Traditional DNS queries are transmitted in plaintext, exposing your browsing history to potential monitoring, hijacking, or tampering. DNS-over-HTTPS (DoH) addresses these vulnerabilities by:

-   **Encrypting DNS traffic** - Protecting against network surveillance and man-in-the-middle attacks
-   **Enhancing privacy** - Preventing ISPs and network operators from inspecting DNS query contents
-   **Strengthening security** - Mitigating DNS poisoning, spoofing, and injection attacks
-   **Circumventing censorship** - Bypassing DNS-based content blocking and network restrictions

## Core Features

-   🔄 **Protocol Conversion**

    -   Efficient transformation of UDP/53 and TCP/53 DNS requests to DoH (RFC 8484 compliant)
    -   Comprehensive support for both GET and POST HTTP methods
    -   Versatile content format handling, including `application/dns-message` and `application/dns-json`

-   🧠 **Intelligent Routing**

    -   **Advanced Domain Matching** - Route DNS queries with precision based on flexible pattern matching:
        -   Exact domain matching for specific hostnames
        -   Wildcard domain matching (e.g., `*.example.com`) for domain families
        -   Regular expression matching for complex pattern needs
    -   **Granular Action Control** - Define precise handling for each matched domain:
        -   Forward queries to designated upstream DoH groups
        -   Block malicious or unwanted domains (return NXDOMAIN)

-   🌐 **Sophisticated Upstream Management**

    -   **Strategic Grouping** - Organize DoH servers into independently configured logical groups
    -   **Advanced Load Balancing** - Implement optimal distribution strategies for each server group:
        -   Round Robin (RR) - Distribute requests sequentially for even load distribution
        -   Weighted Round Robin (WRR) - Prioritize higher-capacity servers with custom weighting
        -   Random Selection - Enhance privacy through non-deterministic server selection
    -   **Comprehensive Authentication** - Secure communication with private DoH providers:
        -   HTTP Basic Authentication for username/password authentication
        -   Bearer Token Authentication for API key or OAuth token authentication
    -   **Optimized Resource Management** - Shared HTTP client connection pool across all upstream groups for maximized efficiency

-   ⚡ **Performance Optimization**

    -   **Intelligent Caching** - Sophisticated DNS caching engine dramatically reduces latency and upstream load
        -   **Positive Caching** - Store successful DNS responses for rapid subsequent resolution
        -   **Negative Caching** - Cache error responses to prevent redundant queries for non-existent domains
        -   **Configurable TTL Management** - Set customized time-to-live values for optimal cache freshness
    -   **Connection Pooling** - Efficient HTTP connection reuse for reduced overhead and improved performance
    -   **TTL Optimization** - Configurable minimum and maximum TTL boundaries for optimized cache behavior

-   🔁 **Enterprise-Grade Reliability**

    -   **Resilient Request Handling** - Automatically retry failed DoH requests with configurable attempt limits
    -   **Comprehensive Timeout Management** - Fine-tune connection and request timeout parameters for optimal reliability

-   ⚙️ **Advanced Management Capabilities**
    -   **Structured YAML Configuration** - Clean, human-readable configuration format
    -   **Strict Configuration Validation** - Robust configuration checks at startup or in test mode
    -   **Comprehensive Health Monitoring** - Complete integration with modern monitoring infrastructure
    -   **Rich Prometheus Metrics** - Detailed performance and operational metrics via the `/metrics` endpoint

## Architecture

Load Ants implements a modular, microservices-inspired architecture with the following key components:

-   **Server Module**: Highly efficient UDP/TCP listeners that receive and parse traditional DNS queries
-   **Routing Module**: Sophisticated pattern-matching engine that determines optimal processing strategy
-   **Upstream Management Module**: Advanced client that manages DoH server communication, load balancing, and authentication
-   **Cache Module**: Performance-optimized storage system for DNS responses to minimize latency and upstream load
-   **Processor Module**: Orchestration layer that coordinates component interactions throughout the DNS resolution workflow

![architecture](./images/architecture.png)

## Prometheus Metrics

Load Ants delivers comprehensive Prometheus metrics for real-time monitoring of service performance, health status, and operational efficiency. These metrics are exposed via the standard `/metrics` endpoint and can be collected by Prometheus or any compatible monitoring platform.

![metrics](./images/metrics.png)

### DNS Performance Metrics

-   **loadants_dns_requests_total** (Counter) - Total DNS requests processed, segmented by protocol (UDP/TCP)
-   **loadants_dns_request_duration_seconds** (Histogram) - DNS request processing latency, segmented by protocol and query type
-   **loadants_dns_request_errors_total** (Counter) - DNS request processing errors, segmented by error category

### Cache Efficiency Metrics

-   **loadants_cache_entries** (Gauge) - Current DNS cache entry count
-   **loadants_cache_capacity** (Gauge) - Maximum configured cache capacity
-   **loadants_cache_operations_total** (Counter) - Cache operations by type (hit, miss, insert, eviction, expiration)
-   **loadants_cache_ttl_seconds** (Histogram) - Distribution of cache entry TTLs, segmented by TTL source
-   **loadants_negative_cache_hits_total** (Counter) - Negative cache hit count for efficiency analysis

### DNS Query Metrics

-   **loadants_dns_query_type_total** (Counter) - DNS queries by record type (A, AAAA, MX, etc.)
-   **loadants_dns_response_codes_total** (Counter) - DNS responses by response code (NOERROR, NXDOMAIN, etc.)

### Upstream Resolver Metrics

-   **loadants_upstream_requests_total** (Counter) - Requests to upstream DoH resolvers, segmented by group and server
-   **loadants_upstream_errors_total** (Counter) - Upstream DoH resolver errors, segmented by error type, group, and server
-   **loadants_upstream_duration_seconds** (Histogram) - Upstream DoH query latency, segmented by group and server

### DNS Routing Metrics

-   **loadants_route_matches_total** (Counter) - Routing rule matches, segmented by rule type and target group
-   **loadants_route_rules_count** (Gauge) - Active routing rules count, segmented by rule type

These comprehensive metrics enable detailed performance analysis, rapid troubleshooting, and data-driven optimization of Load Ants deployments.

## API Endpoints

Load Ants provides streamlined API endpoints for DNS resolution and operational monitoring:

### DNS Endpoints

-   **UDP and TCP Port 53**
    -   _Description_: Standard DNS service ports compliant with RFC 1035
    -   _Protocol_: DNS over UDP/TCP
    -   _Usage_: Compatible with all standard DNS clients, applications, and operating systems

### Admin Endpoints

-   **GET /health**

    -   _Description_: Health check endpoint for monitoring systems and Kubernetes liveness/readiness probes
    -   _Returns_: 200 OK when service is operational
    -   _Usage_: `curl http://localhost:8080/health`

-   **GET /metrics**

    -   _Description_: Prometheus-compatible metrics endpoint exposing operational telemetry
    -   _Content Type_: text/plain; version=0.0.4
    -   _Usage_: `curl http://localhost:8080/metrics`

-   **POST /api/cache/refresh**
    -   _Description_: Administrative endpoint to clear the DNS cache
    -   _Returns_: JSON response indicating success or error
    -   _Usage_: `curl -X POST http://localhost:8080/api/cache/refresh`
    -   _Response Example_: `{"status":"success","message":"DNS cache has been cleared"}`

All endpoints implement standard HTTP status codes:

-   200: Successful operation
-   400: Bad request (e.g., when cache is not enabled)
-   500: Internal server error

## Use Cases

Load Ants delivers exceptional value in diverse deployment scenarios:

-   **Enterprise Networks**: Implement centralized, secure DNS resolution with encrypted traffic, granular routing policies, and comprehensive monitoring
-   **Privacy-Focused Users**: Protect browsing history from ISP surveillance, bypass DNS censorship, and enhance online privacy
-   **Cloud-Native Environments**: Deploy as a sidecar or dedicated service in Kubernetes clusters for secure, high-performance DNS resolution
-   **Content Filtering**: Implement domain-based access controls by selectively blocking or redirecting specific DNS queries

## Installation

### Requirements

-   Rust toolchain 1.56+ (for building from source)
-   Privileged access (for binding to port 53)

### Building from Source

1. Clone the repository:

    ```bash
    git clone https://github.com/shengyanli1982/load-ants.git
    cd load-ants
    ```

2. Build the optimized release binary:

    ```bash
    cargo build --release
    ```

3. Pre-compiled binaries for major platforms are available on the [releases page](https://github.com/shengyanli1982/load-ants/releases).

### Deploying with Docker

Docker provides a frictionless deployment option without installing Rust or dependencies directly:

1. Create a configuration directory:

    ```bash
    mkdir -p ./load-ants-config
    ```

2. Prepare your configuration file:

    ```bash
    cp config.default.yaml ./load-ants-config/config.yaml
    # Edit the configuration file to suit your environment
    ```

3. Launch the Load Ants container:

    ```bash
    docker run -d \
      --name load-ants \
      -p 53:53/udp \
      -p 53:53/tcp \
      -p 8080:8080 \
      -v $(pwd)/load-ants-config:/etc/load-ants \
      shengyanli1982/load-ants:latest -c /etc/load-ants/config.yaml
    ```

4. Monitor container logs:

    ```bash
    docker logs load-ants
    ```

5. Manage the container:
    ```bash
    docker stop load-ants
    docker rm load-ants
    ```

### Kubernetes Deployment

For production environments, Kubernetes offers superior scalability, resilience, and operational management:

1. Create a ConfigMap with your configuration:

    ```yaml
    # configmap.yaml
    apiVersion: v1
    kind: ConfigMap
    metadata:
        name: load-ants-config
        namespace: dns
    data:
        config.yaml: |
            server:
              listen_udp: "0.0.0.0:53"
              listen_tcp: "0.0.0.0:53"
            health:
              listen: "0.0.0.0:8080"
            cache:
              enabled: true
              max_size: 10000
              min_ttl: 60
              max_ttl: 3600
              negative_ttl: 300
            # Additional configuration as needed...
    ```

2. Define a Deployment resource:

    ```yaml
    # deployment.yaml
    apiVersion: apps/v1
    kind: Deployment
    metadata:
        name: load-ants
        namespace: dns
        labels:
            app: load-ants
    spec:
        replicas: 2
        selector:
            matchLabels:
                app: load-ants
        template:
            metadata:
                labels:
                    app: load-ants
                annotations:
                    prometheus.io/scrape: "true"
                    prometheus.io/port: "8080"
                    prometheus.io/path: "/metrics"
            spec:
                containers:
                    - name: load-ants
                      image: shengyanli1982/load-ants:latest
                      args: ["-c", "/etc/load-ants/config.yaml"]
                      ports:
                          - containerPort: 53
                            name: dns-udp
                            protocol: UDP
                          - containerPort: 53
                            name: dns-tcp
                            protocol: TCP
                          - containerPort: 8080
                            name: metrics
                      volumeMounts:
                          - name: config-volume
                            mountPath: /etc/load-ants
                      resources:
                          limits:
                              memory: "256Mi"
                              cpu: "500m"
                          requests:
                              memory: "128Mi"
                              cpu: "100m"
                      livenessProbe:
                          httpGet:
                              path: /health
                              port: 8080
                          initialDelaySeconds: 5
                          periodSeconds: 10
                      readinessProbe:
                          httpGet:
                              path: /health
                              port: 8080
                          initialDelaySeconds: 3
                          periodSeconds: 5
                volumes:
                    - name: config-volume
                      configMap:
                          name: load-ants-config
    ```

3. Create a Service resource:

    ```yaml
    # service.yaml
    apiVersion: v1
    kind: Service
    metadata:
        name: load-ants
        namespace: dns
    spec:
        selector:
            app: load-ants
        ports:
            - port: 53
              name: dns-udp
              protocol: UDP
              targetPort: 53
            - port: 53
              name: dns-tcp
              protocol: TCP
              targetPort: 53
        type: ClusterIP
    ```

4. Apply the configuration to your cluster:

    ```bash
    kubectl create namespace dns
    kubectl apply -f configmap.yaml
    kubectl apply -f deployment.yaml
    kubectl apply -f service.yaml
    ```

5. Verify deployment status:
    ```bash
    kubectl -n dns get pods
    kubectl -n dns get svc
    ```

### Running as a System Service

#### Linux (systemd)

1. Create a service unit file at `/etc/systemd/system/load-ants.service`:

    ```ini
    [Unit]
    Description=Load Ants DNS-over-HTTPS Proxy Service
    Documentation=https://github.com/shengyanli1982/load-ants
    After=network.target
    Wants=network-online.target

    [Service]
    Type=simple
    ExecStart=/usr/local/bin/load-ants -c /etc/load-ants/config.yaml
    Restart=on-failure
    RestartSec=5
    User=root
    LimitNOFILE=65536

    # Security enhancements (optional)
    CapabilityBoundingSet=CAP_NET_BIND_SERVICE
    AmbientCapabilities=CAP_NET_BIND_SERVICE
    NoNewPrivileges=true

    [Install]
    WantedBy=multi-user.target
    ```

2. Create configuration directory and file:

    ```bash
    mkdir -p /etc/load-ants
    cp config.default.yaml /etc/load-ants/config.yaml
    # Customize configuration as needed
    ```

3. Enable and start the service:
    ```bash
    systemctl daemon-reload
    systemctl enable load-ants
    systemctl start load-ants
    systemctl status load-ants
    ```

## Usage

### Command Line Arguments

```
load-ants [OPTIONS]

Options:
    -c, --config <PATH>    Configuration file path (default: ./config.yaml)
    -t, --test             Validate configuration and exit
    -h, --help             Display help information
    -V, --version          Display version information
```

### Quick Start Guide

1. Create a configuration file based on the template:

    ```bash
    cp config.default.yaml config.yaml
    ```

2. Edit the configuration file to specify your preferred DoH providers and routing rules

3. Launch Load Ants with appropriate permissions:

    ```bash
    sudo ./load-ants -c config.yaml
    ```

4. Verify the service is operational:
    ```bash
    dig @127.0.0.1 example.com
    curl http://localhost:8080/health
    ```

## Configuration Guide

Load Ants uses YAML-formatted configuration files for maximum flexibility and readability. Below is a comprehensive reference of all configuration options:

### Server Configuration (server)

| Parameter   | Type    | Default        | Description                           | Valid Range          |
| ----------- | ------- | -------------- | ------------------------------------- | -------------------- |
| listen_udp  | String  | "127.0.0.1:53" | UDP DNS listening address and port    | Valid IP:port format |
| listen_tcp  | String  | "127.0.0.1:53" | TCP DNS listening address and port    | Valid IP:port format |
| tcp_timeout | Integer | 10             | TCP connection idle timeout (seconds) | 1-3600               |

### Health Check Configuration (health)

| Parameter | Type   | Default          | Description                                     | Valid Range          |
| --------- | ------ | ---------------- | ----------------------------------------------- | -------------------- |
| listen    | String | "127.0.0.1:8080" | Health check service listening address and port | Valid IP:port format |

### Cache Configuration (cache)

| Parameter    | Type    | Default | Description                  | Valid Range |
| ------------ | ------- | ------- | ---------------------------- | ----------- |
| enabled      | Boolean | true    | Enable caching               | true/false  |
| max_size     | Integer | 10000   | Maximum cache entries        | 10-1000000  |
| min_ttl      | Integer | 60      | Minimum TTL (seconds)        | 1-86400     |
| max_ttl      | Integer | 3600    | Maximum TTL (seconds)        | 1-86400     |
| negative_ttl | Integer | 300     | Negative cache TTL (seconds) | 1-86400     |

The cache system provides sophisticated DNS response caching with fine-grained control:

-   **enabled**: Master switch for the caching subsystem
-   **max_size**: Maximum number of cached DNS records (memory usage scales linearly)
-   **min_ttl**: Floor value for DNS response TTLs (overrides shorter TTLs to reduce cache churn)
-   **max_ttl**: Ceiling value for DNS response TTLs (caps excessively long TTLs for improved freshness)
-   **negative_ttl**: Dedicated TTL for negative responses (NXDOMAIN, ServFail, etc.)

Negative caching significantly enhances performance by temporarily storing error responses, preventing repeated upstream queries for non-existent or problematic domains, thereby reducing latency and conserving upstream bandwidth.

### HTTP Client Configuration (http_client)

| Parameter       | Type    | Default | Description                                  | Valid Range      |
| --------------- | ------- | ------- | -------------------------------------------- | ---------------- |
| connect_timeout | Integer | 5       | Connection timeout (seconds)                 | 1-120            |
| request_timeout | Integer | 10      | Request timeout (seconds)                    | 1-1200           |
| idle_timeout    | Integer | 60      | Idle connection timeout (seconds) (optional) | 5-1800           |
| keepalive       | Integer | 60      | TCP Keepalive (seconds) (optional)           | 5-600            |
| agent           | String  | -       | HTTP User-Agent (optional)                   | Non-empty string |

### Upstream DoH Server Group Configuration (upstream_groups)

| Parameter | Type   | Default | Description                    | Valid Range                        |
| --------- | ------ | ------- | ------------------------------ | ---------------------------------- |
| name      | String | -       | Group name                     | Non-empty string                   |
| strategy  | String | -       | Load balancing strategy        | "roundrobin", "weighted", "random" |
| servers   | Array  | -       | Server list                    | At least one server                |
| retry     | Object | -       | Retry configuration (optional) | -                                  |
| proxy     | String | -       | HTTP/SOCKS5 proxy (optional)   | Valid proxy URL                    |

#### Server Configuration (servers)

| Parameter    | Type    | Default   | Description                             | Valid Range                 |
| ------------ | ------- | --------- | --------------------------------------- | --------------------------- |
| url          | String  | -         | DoH server URL                          | Valid HTTP(S) URL with path |
| weight       | Integer | 1         | Weight (only for weighted strategy)     | 1-65535                     |
| method       | String  | "post"    | DoH request method                      | "get", "post"               |
| content_type | String  | "message" | DoH content type                        | "message", "json"           |
| auth         | Object  | -         | Authentication configuration (optional) | -                           |

#### Authentication Configuration (auth)

| Parameter | Type   | Default | Description                    | Valid Range       |
| --------- | ------ | ------- | ------------------------------ | ----------------- |
| type      | String | -       | Authentication type            | "basic", "bearer" |
| username  | String | -       | Username (only for basic auth) | Non-empty string  |
| password  | String | -       | Password (only for basic auth) | Non-empty string  |
| token     | String | -       | Token (only for bearer auth)   | Non-empty string  |

#### Retry Configuration (retry)

| Parameter | Type    | Default | Description              | Valid Range |
| --------- | ------- | ------- | ------------------------ | ----------- |
| attempts  | Integer | -       | Number of retry attempts | 1-100       |
| delay     | Integer | -       | Initial delay (seconds)  | 1-120       |

### Routing Rules Configuration (static_rules)

| Parameter | Type   | Default | Description                                             | Valid Range                      |
| --------- | ------ | ------- | ------------------------------------------------------- | -------------------------------- |
| match     | String | -       | Match type                                              | "exact", "wildcard", "regex"     |
| patterns  | Array  | -       | Match patterns                                          | Non-empty array of strings       |
| action    | String | -       | Routing action                                          | "forward", "block"               |
| target    | String | -       | Target upstream group (required when action is forward) | Name of a defined upstream group |

Load Ants implements a sophisticated priority-based routing system:

1. **Exact Matching** (highest priority) - Perfect match of the complete domain name (e.g., `example.com`)
2. **Wildcard Matching** - Domain pattern matching with wildcards (e.g., `*.example.com`)
3. **Regular Expression Matching** - Advanced pattern matching using regex (e.g., `^(mail|smtp)\\.example\\.com$`)
4. **Global Wildcard** (lowest priority) - Catch-all rule using the wildcard pattern (`*`)

For optimal routing configuration, rules should be ordered from most specific to least specific, with the global wildcard rule typically serving as the final fallback option.

### Configuration Example

```yaml
# Load Ants Configuration Example

# Server listening configuration
server:
    listen_udp: "0.0.0.0:53" # UDP listening address and port
    listen_tcp: "0.0.0.0:53" # TCP listening address and port
    tcp_timeout: 10 # TCP connection timeout in seconds

# Health check and metrics endpoint
health:
    listen: "0.0.0.0:8080" # Health/metrics server address and port

# DNS cache configuration
cache:
    enabled: true # Enable DNS caching
    max_size: 10000 # Maximum cache entries
    min_ttl: 60 # Minimum TTL in seconds
    max_ttl: 3600 # Maximum TTL in seconds
    negative_ttl: 300 # TTL for negative responses (NXDOMAIN, etc.)

# HTTP client settings
http_client:
    connect_timeout: 5 # Connection timeout in seconds
    request_timeout: 10 # Request timeout in seconds
    idle_timeout: 60 # Connection idle timeout in seconds
    keepalive: 60 # TCP keepalive in seconds
    agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"

# Upstream DoH server groups
upstream_groups:
    # Google Public DNS group with round-robin load balancing
    - name: "google_public"
      strategy: "roundrobin" # Load balancing strategy
      servers:
          - url: "https://dns.google/dns-query"
          - url: "https://8.8.4.4/dns-query"
            method: "get" # HTTP method (get or post)
            content_type: "message" # Content type (message or json)
      retry:
          attempts: 3 # Number of retry attempts
          delay: 1 # Initial delay between retries in seconds
      proxy: "http://user:pass@proxyserver:port" # Optional HTTP/SOCKS5 proxy

    # Cloudflare DNS group with random server selection
    - name: "cloudflare_secure"
      strategy: "random" # Random server selection for enhanced privacy
      servers:
          - url: "https://cloudflare-dns.com/dns-query"
            method: "post"
          - url: "https://1.0.0.1/dns-query"
            method: "get"
            content_type: "json"

    # NextDNS group with weighted load balancing
    - name: "nextdns_weighted"
      strategy: "weighted" # Weighted distribution
      servers:
          - url: "https://dns.nextdns.io/YOUR_CONFIG_ID"
            weight: 70 # 70% of requests go to this server
            auth:
                type: "bearer" # Authentication type
                token: "YOUR_API_KEY_OR_TOKEN" # API key or token
          - url: "https://dns2.nextdns.io/YOUR_CONFIG_ID"
            weight: 30 # 30% of requests go to this server
      retry:
          attempts: 2
          delay: 2

# DNS routing rules (static rules)
static_rules:
    # Block specific advertising domains
    - match: "exact"
      patterns: ["ads.example.com", "ads2.example.com"] # Multiple patterns in an array
      action: "block" # Return NXDOMAIN response

    # Route internal corporate domains to internal resolver
    - match: "wildcard"
      patterns: ["*.corp.local", "*.corp.internal"] # Multiple patterns in an array
      action: "forward"
      target: "internal_doh" # Target upstream group

    # Route CDN domains using regex pattern matching
    - match: "regex"
      patterns: ["^(video|audio)-cdn\\..+\\.com$"]
      action: "forward"
      target: "google_public"

    # Default rule: forward all other traffic to Google Public DNS
    - match: "wildcard"
      patterns: ["*"]
      action: "forward"
      target: "google_public"
```

## License

[MIT License](LICENSE)

## Acknowledgements

-   Thanks to all contributors who have helped improve the Load Ants project
-   This project builds upon modern DNS security standards and DoH implementation technologies
-   Inspired by real-world needs for flexible, secure, and efficient DNS routing solutions
