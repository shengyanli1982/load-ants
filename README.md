English | [中文](./README_CN.md)

<div align="center">
    <h1>LOAD ANTS</h1>
    <h4>🐜🐜🐜 A lightweight DNS forwarder converting UDP/TCP queries to DoH.</h4></br>
    <img src="./images/logo.png" alt="logo" width="600">
</div>

## Introduction

**Load Ants** is a high-performance, versatile DNS proxy service that converts traditional UDP/TCP DNS queries to DNS-over-HTTPS (DoH). It acts as an intermediary between your clients using standard DNS protocols and modern secure DoH providers, enabling enhanced privacy, security, and flexible routing capabilities.

### Why DNS-over-HTTPS?

Traditional DNS queries are transmitted in plaintext, exposing your browsing history to potential monitoring, hijacking, or manipulation. DoH addresses these issues by:

-   **Encrypting DNS traffic** - Prevents snooping by network intermediaries
-   **Enhancing privacy** - Hides DNS queries from ISPs and other network observers
-   **Improving security** - Reduces vulnerability to DNS poisoning and spoofing attacks
-   **Bypassing censorship** - Helps circumvent DNS-based blocking techniques

## Key Features

-   🔄 **Protocol Conversion**

    -   Seamlessly transforms UDP/53 and TCP/53 DNS requests to DoH (RFC 8484)
    -   Fully supports both GET and POST HTTP methods
    -   Handles multiple content formats including `application/dns-message` and `application/dns-json`

-   🧠 **Intelligent Routing**

    -   **Flexible Matching** - Route DNS queries based on domain patterns:
        -   Exact domain name matching
        -   Wildcard domain matching (`*.example.com`)
        -   Regular expression domain matching
    -   **Custom Actions** - Define what happens for each match:
        -   Forward to specific upstream DoH group
        -   Block queries (responds with NXDOMAIN)

-   🌐 **Flexible Upstream Management**

    -   **Grouping** - Organize DoH servers into logical groups with independent settings
    -   **Load Balancing** - Configure per-group balancing strategies:
        -   Round Robin (RR) - Equal distribution among servers
        -   Weighted Round Robin (WRR) - Prioritize servers by capacity
        -   Random - Non-deterministic selection for enhanced privacy
    -   **Authentication Support** - Secure communication with private DoH providers:
        -   HTTP Basic Authentication
        -   Bearer Token Authentication
    -   **Resource Optimization** - All upstream groups share HTTP client pool for efficiency

-   ⚡ **Performance Enhancements**

    -   **Intelligent Caching** - Built-in DNS cache reduces latency and upstream load
    -   **Connection Pooling** - Reuses HTTP connections for efficiency
    -   **Adjustable TTL** - Configure minimum and maximum TTL for cached responses

-   🔁 **Reliability**

    -   **Retry Mechanism** - Automatically retry failed DoH requests with configurable attempts
    -   **Customizable Timeouts** - Fine-tune connect and request timeouts

-   ⚙️ **Administration**
    -   **YAML Configuration** - Simple, human-readable configuration
    -   **Configuration Validation** - Strict validation at startup or with test mode
    -   **Health Endpoint** - Monitoring integration for operations teams
    -   **Prometheus Metrics** - Comprehensive metrics for monitoring via `/metrics` endpoint

## Architecture

Load Ants follows a modular architecture with the following key components:

-   **Server**: UDP/TCP listeners accepting traditional DNS queries
-   **Router**: Matches domain names against rules to determine processing action
-   **Upstream Manager**: Handles communication with DoH servers, including load balancing and authentication
-   **Cache**: Stores DNS responses to improve performance and reduce upstream load
-   **Handler**: Processes DNS queries by coordinating between other components

![architecture](./images/architecture.png)

## Prometheus Metrics

Load Ants provides comprehensive Prometheus metrics to monitor the performance, health, and operational status of the service. These metrics are exposed via the `/metrics` endpoint, which can be scraped by Prometheus or other compatible monitoring systems.

![metrics](./images/metrics.png)

### DNS Performance Metrics

-   **loadants_dns_requests_total** (counter) - Total DNS requests processed by the proxy, labeled by protocol (UDP/TCP)
-   **loadants_dns_request_duration_seconds** (histogram) - DNS request processing duration in seconds, labeled by protocol and query type
-   **loadants_dns_request_errors_total** (counter) - Total DNS request processing errors, labeled by error type

### Cache Efficiency Metrics

-   **loadants_cache_entries** (gauge) - Current number of DNS cache entries
-   **loadants_cache_capacity** (gauge) - Maximum capacity of the DNS cache
-   **loadants_cache_operations_total** (counter) - Total cache operations, labeled by operation type (hit, miss, insert, evict, expire)
-   **loadants_cache_ttl_seconds** (histogram) - TTL distribution of DNS cache entries in seconds

### DNS Query Metrics

-   **loadants_dns_query_type_total** (counter) - Total DNS queries by record type (A, AAAA, MX, etc.)
-   **loadants_dns_response_codes_total** (counter) - Total DNS responses by response code (RCODE)

### Upstream Resolver Metrics

-   **loadants_upstream_requests_total** (counter) - Total requests sent to upstream DoH resolvers, labeled by group and server
-   **loadants_upstream_errors_total** (counter) - Total upstream DoH resolver errors, labeled by error type, group and server
-   **loadants_upstream_duration_seconds** (histogram) - Upstream DoH query duration in seconds, labeled by group and server

### DNS Routing Metrics

-   **loadants_route_matches_total** (counter) - Total routing rule matches, labeled by rule type (exact, wildcard, regex) and target group
-   **loadants_route_rules_count** (gauge) - Current number of active routing rules, labeled by rule type (exact, wildcard, regex)

These metrics enable detailed monitoring and analysis of Load Ants performance and behavior, making it easier to identify issues, optimize configurations, and ensure the service meets your performance requirements.

## API Endpoints

Load Ants provides the following HTTP API endpoints for DNS resolution and service monitoring:

### DNS Endpoints

-   **UDP and TCP Port 53**
    -   _Description_: Standard DNS ports for receiving traditional DNS queries
    -   _Protocol_: DNS over UDP/TCP (RFC 1035)
    -   _Usage_: Applications and systems using standard DNS resolution will send queries to these ports

### Monitoring and Health Endpoints

-   **GET /health**

    -   _Description_: Health check endpoint for monitoring services and Kubernetes probes
    -   _Returns_: 200 OK when service is healthy
    -   _Usage_: `curl http://localhost:8080/health`

-   **GET /metrics**
    -   _Description_: Prometheus metrics endpoint exposing performance and operational statistics
    -   _Content Type_: text/plain
    -   _Usage_: `curl http://localhost:8080/metrics`

These endpoints adhere to standard HTTP status codes:

-   200: Successful query/operation
-   500: Server error during processing

## Use Cases

Load Ants is well-suited for the following scenarios:

-   **Enterprise/Internal Networks**: Centralize DNS resolution, enforce encryption, and implement internal name resolution policies
-   **Personal Users/Developers**: Bypass ISP DNS restrictions/poisoning, improve privacy, and flexibly control specific domain resolution
-   **Cloud Environments**: Deploy as a sidecar or standalone service providing DNS resolution capabilities

## Installation

### Prerequisites

-   Administrative/root privileges (for binding to port 53)

### From Source

1. Clone the repository:

    ```bash
    git clone https://github.com/yourusername/load-ants.git
    cd load-ants
    ```

2. Build the application:

    ```bash
    cargo build --release
    ```

3. The compiled binary can be downloaded from the [releases](https://github.com/shengyanli1982/load-ants/releases) page.

### Using Docker

Docker provides a simple way to run Load Ants without installing Rust or dependencies directly on your system.

1. Create a directory for your configuration:

    ```bash
    mkdir -p ./load-ants-config
    ```

2. Create a configuration file:

    ```bash
    cp config.default.yaml ./load-ants-config/config.yaml
    # Edit the configuration file to suit your needs
    ```

3. Run Load Ants as a Docker container:

    ```bash
    docker run -d \
      --name load-ants \
      -p 53:53/udp \
      -p 53:53/tcp \
      -p 8080:8080 \
      -v $(pwd)/load-ants-config:/etc/load-ants \
      yourusername/load-ants:latest -c /etc/load-ants/config.yaml
    ```

4. Check container logs:

    ```bash
    docker logs load-ants
    ```

5. Stop the container:
    ```bash
    docker stop load-ants
    docker rm load-ants
    ```

### Kubernetes Deployment

For production environments, Kubernetes provides scaling, high availability, and easier management.

1. Create a ConfigMap for the configuration:

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
            # Add the rest of your configuration...
    ```

2. Create a Deployment:

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
            spec:
                containers:
                    - name: load-ants
                      image: yourusername/load-ants:latest
                      args: ["-c", "/etc/load-ants/config.yaml"]
                      ports:
                          - containerPort: 53
                            name: dns-udp
                            protocol: UDP
                          - containerPort: 53
                            name: dns-tcp
                            protocol: TCP
                          - containerPort: 8080
                            name: health
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
                volumes:
                    - name: config-volume
                      configMap:
                          name: load-ants-config
    ```

3. Create a Service:

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

4. Apply the configurations:

    ```bash
    kubectl create namespace dns
    kubectl apply -f configmap.yaml
    kubectl apply -f deployment.yaml
    kubectl apply -f service.yaml
    ```

5. Check the deployment status:
    ```bash
    kubectl -n dns get pods
    kubectl -n dns get svc
    ```

### Using as a Service

#### Linux (systemd)

1. Create a service file `/etc/systemd/system/load-ants.service`:

    ```ini
    [Unit]
    Description=Load Ants DNS to DoH Proxy
    After=network.target

    [Service]
    ExecStart=/path/to/load-ants -c /etc/load-ants/config.yaml
    Restart=on-failure
    User=root

    [Install]
    WantedBy=multi-user.target
    ```

2. Create configuration directory and file:

    ```bash
    mkdir -p /etc/load-ants
    cp config.default.yaml /etc/load-ants/config.yaml
    # Edit the config file to match your needs
    ```

3. Enable and start the service:
    ```bash
    systemctl enable load-ants
    systemctl start load-ants
    ```

## Usage

### Command Line Options

```
load-ants [OPTIONS]

OPTIONS:
    -c, --config <PATH>    Path to configuration file (default: ./config.yaml)
    -t, --test             Test configuration file and exit
    -h, --help             Print help information
    -V, --version          Print version information
```

### Example

1. Create a configuration file based on the template:

    ```bash
    cp config.default.yaml config.yaml
    ```

2. Edit the configuration file to suit your needs

3. Run Load Ants with your configuration:

    ```bash
    sudo ./load-ants -c config.yaml
    ```

4. Test the service by using it as a DNS server:
    ```bash
    dig @127.0.0.1 example.com
    ```

## Configuration

Load Ants is configured using a YAML file. Below is an explanation of the key sections:

### Server Configuration

```yaml
server:
    listen_udp: "0.0.0.0:53" # UDP listening address and port
    listen_tcp: "0.0.0.0:53" # TCP listening address and port
```

### Health Check

```yaml
health:
    listen: "0.0.0.0:8080" # Health check server listen address and port
```

### Cache Settings

```yaml
cache:
    enabled: true
    max_size: 10000 # Maximum number of entries (10-1000000)
    min_ttl: 60 # Minimum TTL in seconds (1-86400)
    max_ttl: 3600 # Maximum TTL in seconds (1-86400)
```

### HTTP Client Settings

```yaml
http_client:
    connect_timeout: 5 # Connection timeout in seconds (1-120)
    request_timeout: 10 # Request timeout in seconds (1-1200)
    idle_timeout: 60 # Idle connection timeout in seconds (5-1800)
    keepalive: 60 # TCP keepalive in seconds (5-600)
    agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
```

### Upstream DoH Server Groups

```yaml
upstream_groups:
    - name: "google_public"
      strategy: "roundrobin" # Strategy: roundrobin, weighted, random
      servers:
          - url: "https://dns.google/dns-query"
          - url: "https://8.8.4.4/dns-query"
            method: "get" # Optional: get or post, default is post
            content_type: "message" # Optional: message or json, default is message
      retry:
          attempts: 3 # Number of retry attempts (1-100)
          delay: 1 # Initial delay in seconds (1-120)
      proxy: "http://user:pass@proxyserver:port" # Optional proxy

    - name: "secure_dns"
      strategy: "weighted"
      servers:
          - url: "https://example-doh.com/dns-query"
            weight: 70 # Weight for weighted strategy (1-65535)
            auth:
                type: "bearer" # Authentication type: basic or bearer
                token: "YOUR_API_TOKEN" # Token for bearer auth
          - url: "https://another-doh.com/dns-query"
            weight: 30
            auth:
                type: "basic"
                username: "user"
                password: "pass"
```

### Routing Rules

Load Ants uses a priority-based matching system for DNS routing:

1. **Exact Match** (highest priority) - Direct match for full domain names (e.g., `example.com`)
2. **Specific Wildcard Match** - Matches domains using wildcards (e.g., `*.example.com`)
3. **Regex Match** - Matches domains using regular expressions (e.g., `^(mail|audio)\\.google\\.com$`)
4. **Global Wildcard Match** (lowest priority) - The catch-all rule (`*`) that matches any domain

When configuring routing rules, keep this priority order in mind. The global wildcard (`*`) should typically be placed as the last rule to serve as a default when no other rules match.

```yaml
routing_rules:
    # Block specific domains
    - match: "exact" # Match type: exact, wildcard, regex
      pattern: "ads.example.com" # Pattern to match
      action: "block" # Action: block or forward

    # Route internal domains to specific upstream group
    - match: "wildcard"
      pattern: "*.internal.local"
      action: "forward"
      target: "internal_dns" # Target upstream group

    # Use regex for pattern matching
    - match: "regex"
      pattern: "^ads-.*\\.example\\.com$"
      action: "forward"
      target: "adblock_dns"

    # Default rule (catch-all)
    - match: "wildcard"
      pattern: "*" # Match everything
      action: "forward"
      target: "google_public" # Default upstream group
```

## License

[MIT License](LICENSE)

## Acknowledgments

-   Thanks to all the contributors who have helped shape Load Ants
-   Inspired by modern DoH implementations and the need for flexible DNS routing
