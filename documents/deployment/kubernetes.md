# 在 Kubernetes 上部署

对于需要高可用、可扩展和易于管理的生产环境，将 Load Ants 部署到 Kubernetes 是理想的选择。本指南将引导你完成在 Kubernetes 集群中部署 Load Ants 的完整流程。

### 先决条件

-   一个正在运行的 Kubernetes 集群。
-   `kubectl` 命令行工具已配置并连接到你的集群。
-   一个 Docker 镜像仓库（如 Docker Hub, GHCR, ECR）的账户，用于存放你的自定义镜像（如果需要）。

---

### 步骤一：创建 Docker 镜像 (可选)

官方在 `ghcr.io` 上提供了预构建的 Load Ants 镜像 (`ghcr.io/shengyanli1982/load-ants-<arch>:latest`)。在大多数情况下，你可以直接使用此镜像。

但是，如果你对代码进行了自定义修改，你需要构建自己的 Docker 镜像并将其推送到镜像仓库。

```bash
# 假设你的项目根目录有 Dockerfile
# 1. 构建镜像
docker build -t your-repo/load-ants:latest .

# 2. 推送镜像到仓库
docker push your-repo/load-ants:latest
```

> **注意**: 请将 `your-repo/load-ants:latest` 替换为你的实际镜像仓库地址和标签。

---

### 步骤二：创建命名空间

为了更好地组织和隔离资源，建议为 DNS 服务创建一个专用的命名空间。

```bash
kubectl create namespace dns
```

后续所有资源都将部署在这个 `dns` 命名空间下。

---

### 步骤三：创建 ConfigMap

Kubernetes 的 `ConfigMap` 用于将配置文件与应用程序解耦。我们将使用它来存储 `config.yaml`。

1.  创建一个名为 `load-ants-configmap.yaml` 的文件：

    ```yaml
    # load-ants-configmap.yaml
    apiVersion: v1
    kind: ConfigMap
    metadata:
        name: load-ants-config
        namespace: dns
    data:
        config.yaml: |
            # 在这里粘贴你的完整 config.yaml 内容
            server:
              listen_udp: "0.0.0.0:53"
              listen_tcp: "0.0.0.0:53"

            admin:
              listen: "0.0.0.0:9000"
              
            cache:
              enabled: true
              max_size: 10000

            upstream_groups:
              - name: "google_public"
                strategy: "random"
                servers:
                  - url: "https://dns.google/dns-query"
                  - url: "https://8.8.4.4/dns-query"

            static_rules:
              - match: "wildcard"
                patterns: ["*"]
                action: "forward"
                target: "google_public"
    ```

2.  应用此 `ConfigMap` 到你的集群：
    ```bash
    kubectl apply -f load-ants-configmap.yaml
    ```

---

### 步骤四：创建 Deployment

`Deployment` 负责管理 Load Ants Pod 的生命周期，确保指定数量的副本正在运行。

1.  创建一个名为 `load-ants-deployment.yaml` 的文件：

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
        replicas: 2 # 根据需求调整副本数以实现高可用
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
                      image: ghcr.io/shengyanli1982/load-ants-<arch>:latest # 使用官方或你自己的镜像
                      args: ["-c", "/etc/load-ants/config.yaml"]
                      ports:
                          - containerPort: 53
                            name: dns-udp
                            protocol: UDP
                          - containerPort: 53
                            name: dns-tcp
                            protocol: TCP
                          - containerPort: 9000
                            name: http-admin
                      volumeMounts:
                          - name: config-volume
                            mountPath: /etc/load-ants # 挂载配置文件
                      resources: # 强烈建议根据实际情况调整资源请求和限制
                          limits:
                              memory: "256Mi"
                              cpu: "500m"
                          requests:
                              memory: "128Mi"
                              cpu: "100m"
                      livenessProbe: # 健康检查: 如果探测失败，K8s会重启容器
                          httpGet:
                              path: /health
                              port: http-admin
                          initialDelaySeconds: 15
                          periodSeconds: 20
                      readinessProbe: # 就绪探针: 如果探测失败，K8s会停止向此Pod发送流量
                          httpGet:
                              path: /health
                              port: http-admin
                          initialDelaySeconds: 5
                          periodSeconds: 10
                volumes:
                    - name: config-volume
                      configMap:
                          name: load-ants-config # 引用上面创建的 ConfigMap
    ```

2.  应用此 `Deployment` 到你的集群：
    ```bash
    kubectl apply -f load-ants-deployment.yaml
    ```

---

### 步骤五：创建 Service

`Service` 为一组 Pod 提供了一个稳定的网络端点（IP 地址和 DNS 名称），以便其他应用可以访问它们。

1.  创建一个名为 `load-ants-service.yaml` 的文件：

    ```yaml
    # load-ants-service.yaml
    apiVersion: v1
    kind: Service
    metadata:
        name: load-ants-svc
        namespace: dns
    spec:
        selector:
            app: load-ants # 匹配 Deployment 中的 Pod 标签
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
              port: 9000
              protocol: TCP
              targetPort: http-admin
        # 根据你的需求选择暴露服务的方式
        type: ClusterIP # (默认) 仅集群内部访问。集群内其他Pod可通过 `load-ants-svc.dns:53` 访问
        # type: LoadBalancer # 如果需要从外部访问，并且你的云提供商支持 (会自动分配公网IP)
        # type: NodePort # 如果需要在每个节点的特定端口上暴露服务
    ```

    > **提示**: `ClusterIP` 是最常见的选择，用于集群内部的 DNS 服务。如果你希望将此 DNS 服务暴露给 VPC 网络或公网，`LoadBalancer` 是更好的选择。

2.  应用此 `Service` 到你的集群：
    ```bash
    kubectl apply -f load-ants-service.yaml
    ```

---

### 步骤六：验证部署

完成以上步骤后，你可以检查所有资源是否正常运行。

1.  **检查 Pod 状态**:

    ```bash
    # 查看 dns 命名空间下的所有 Pod
    kubectl -n dns get pods

    # Pod 状态应为 'Running'
    NAME                         READY   STATUS    RESTARTS   AGE
    load-ants-5f768f4f6-abcde   1/1     Running   0          2m
    load-ants-5f768f4f6-fghij   1/1     Running   0          2m
    ```

2.  **检查 Service 状态**:

    ```bash
    kubectl -n dns get svc load-ants-svc

    # 你会看到分配给 Service 的 IP (ClusterIP 或 外部IP)
    NAME            TYPE        CLUSTER-IP      EXTERNAL-IP   PORT(S)                               AGE
    load-ants-svc   ClusterIP   10.96.100.200   <none>        53/UDP,53/TCP,9000/TCP                5m
    ```

3.  **查看实时日志**:

    ```bash
    # 查看所有 Load Ants Pod 的聚合日志
    kubectl -n dns logs -l app=load-ants -f
    ```

4.  **从集群内部测试 DNS 解析**:
    你可以启动一个临时的 Pod 来测试 DNS 服务是否正常工作。
    ```bash
    kubectl run -it --rm --image=busybox:1.28 dns-test --restart=Never -- nslookup kubernetes.default.svc.cluster.local load-ants-svc.dns
    ```
    如果一切正常，你应该会收到 `kubernetes.default` 的 IP 地址。

---

### 下一步

-   [➡️ 了解安全注意事项](./security.md)
-   [➡️ 了解如何监控服务](./monitoring.md)
-   [➡️ 返回部署总览](./index.md)
