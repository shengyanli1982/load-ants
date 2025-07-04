# 总览

欢迎来到 Load Ants 实操手册！

在前面的章节中，我们已经学习了 Load Ants 的各项独立功能和配置选项。本章节的目标是将这些知识点串联起来，通过一系列完整的、端到端的实例，向你展示如何利用 Load Ants 解决真实世界中的具体问题。

这里的每一个"配方"都是一个独立的、带有明确目标的教程。我们不仅会提供完整的配置文件，还会详细解释其背后的逻辑、部署步骤以及验证方法。

### 可用配方

-   **[终极广告与追踪器拦截](./ad-blocking.md)**

    -   _目标_：搭建一个强大的、全网络范围的广告和恶意追踪器过滤系统。
    -   _涉及模块_：`remote_rules`, `static_rules`, `cache`。

-   **[流媒体服务地理封锁解除](./geo-unblocking.md)**

    -   _目标_：为特定流媒体服务（如 Netflix）配置代理，实现跨区访问，同时保持其他流量直连。
    -   _涉及模块_：`upstream_groups` (带 `proxy` 配置), `routing_rules` (使用 `regex` 匹配)。

-   **[为你的家庭实验室提供内部 DNS](./homelab-dns.md)**
    -   _目标_：为你内部网络的服务（如 `nas.local`, `plex.local`）提供简单、易于管理的 DNS 解析。
    -   _涉及模块_：`static_rules` (使用 `exact` 匹配)。

跟随这些配方，你将能更深入地体会到 Load Ants 的灵活性和强大功能。

---

### 下一步

-   [➡️ 实现广告屏蔽](./ad-blocking.md)
-   [➡️ 实现 GEO 解锁](./geo-unblocking.md)
-   [➡️ 用于家庭实验网络](./homelab-dns.md)
