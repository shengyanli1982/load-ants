# 欢迎来到 Load Ants 的世界

<div align="center">
    <img src="./images/logo.png" alt="logo" width="600">
</div>

**Load Ants** 是一款专为提升网络隐私、安全与灵活性而生的高性能、多功能 DNS 代理服务。支持 **RFC8484** 和 **Google DoH** 标准。

它能够将传统的 UDP/TCP DNS 查询无缝转换为加密的 DNS-over-HTTPS (DoH) 请求，并提供强大的智能路由功能，同时也支持 DoH 代理。

无论你是注重个人隐私的普通用户、家庭网络管理员，还是寻求高效部署方案的开发者，Load Ants 都将成为你手中保障网络连接质量与安全的得力工具。

你可以随时访问我的 Github 仓库，获取更多信息。

-   🌁 Github: [https://github.com/shengyanli1982/load-ants](https://github.com/shengyanli1982/load-ants)

---

### 为什么选择 Load Ants?

在当今的网络环境中，传统的 DNS 查询以明文形式传输，这意味着你的上网记录很容易被窃取、监控甚至篡改。Load Ants 旨在解决这些根本性问题：

-   🔒 **强化隐私保护**: 通过 DoH 加密你的所有 DNS 请求，有效防止网络运营商或中间人窥探你的浏览历史。
-   🛡️ **提升网络安全**: 抵御 DNS 劫持和投毒攻击，确保你访问的是真实、未经修改的网站。
-   🌍 **突破网络限制**: 灵活的路由和代理功能可以帮助你绕过基于 DNS 的封锁，访问更广阔的互联网。
-   🚀 **优化网络性能**: 内置的高效缓存机制能显著降低 DNS 解析延迟，为你带来更快的上网体验。

---

### 核心功能一览

-   🔄 **多协议转换**: 无缝将 `UDP/53` 和 `TCP/53` 请求转换为 DoH，支持向上游发送 `GET` 和 `POST` 方法。
-   🌍 **DoH 代理**: 支持 DoH 代理，可以作为 DoH 服务端使用。 支持 `GET` 和 `POST` 方法。
-   🧠 **智能路由决策**: 可根据精确域名、通配符、正则表达式等多种模式，将查询转发到不同上游或直接拦截。
-   🌐 **灵活上游管理**: 支持将 DoH 服务器分组管理，并为每个组配置轮询、加权或随机等不同负载均衡策略。
-   ⚡ **高性能缓存**: 内置正向与负向缓存，有效降低延迟，减少对上游服务器的请求压力。
-   📜 **远程规则列表**: 支持从 URL 动态加载规则列表（如广告拦截、代理规则），让策略管理更便捷。
-   ⚙️ **强大运维能力**: 提供 Prometheus 指标、健康检查 API 和详细的日志，便于监控和排查问题。

---

### 谁适合使用 Load Ants?

-   **注重隐私的个人用户**: 希望保护个人上网隐私，防止 DNS 数据泄露。
-   **家庭网络管理员**: 希望为整个家庭网络提供去广告、防污染的纯净 DNS 解析服务。
-   **开发者与技术爱好者**: 需要在本地环境测试或使用 DoH，并对 DNS 行为进行精细化控制。
-   **企业网络安全团队**: 寻求一种可集中管理、策略灵活、支持加密的 DNS 解决方案，以增强企业网络安全。

---

### 准备好了吗？

-   [➡️ 从"快速开始"着手](./getting-started/index.md)
