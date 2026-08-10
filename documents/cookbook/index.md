# 总览

欢迎来到 Load Ants 实操手册！

前面的章节介绍了 Load Ants 的各项功能和配置选项。本章把这些知识点串联起来，用几个完整的端到端实例展示如何用 Load Ants 解决真实世界中的具体问题。

这里的每一个“配方”都是一个独立且目标明确的教程：不仅提供完整的配置文件，还会解释背后的逻辑、部署步骤与验证方法。

### 可用配方

- **[终极广告拦截](./ad-blocking.md)**
    - _目标_：搭建覆盖全网络的广告与恶意追踪器过滤系统。
    - _涉及模块_：`remote_rules`、`static_rules`、`cache`。

- **[地理封锁解除](./geo-unblocking.md)**
    - _目标_：为特定流媒体服务（如 Netflix）配置代理，实现跨区访问，同时保持其他流量直连。
    - _涉及模块_：`upstream_groups`（带 `proxy` 配置）、`static_rules`（使用 `regex` 匹配）。

- **[家庭实验室 DNS](./homelab-dns.md)**
    - _目标_：为内部网络中的服务（如 `nas.lan`、`plex.lan`）提供简单、易于管理的 DNS 解析。
    - _涉及模块_：`static_rules`（使用 `regex` 匹配）。

跟随这些配方动手实践，你将更深入地体会到 Load Ants 的灵活性和强大功能。

---

### 下一步

- [➡️ 终极广告拦截](./ad-blocking.md)
- [➡️ 地理封锁解除](./geo-unblocking.md)
- [➡️ 家庭实验室 DNS](./homelab-dns.md)
