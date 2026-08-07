# 总览

本章面向已熟悉基本配置与核心概念的用户，介绍如何在生产环境中部署和管理 Load Ants，覆盖系统服务托管、Docker Compose 编排、安全加固与监控告警。

在这里，你将学到：

- **作为系统服务运行**：如何将 Load Ants 设置为后台持续运行的系统服务，实现进程退出后自动重启与开机自启。
- **使用 Docker Compose**：如何通过 `docker-compose` 编排 Load Ants 与其他应用（如 Web 服务、监控组件）的部署，统一管理多容器的网络配置。
- **安全最佳实践**：如何加固你的 Load Ants 实例，包括保护配置文件、管理敏感信息以及控制管理接口的暴露范围。
- **监控与告警**：如何接入 Prometheus 监控系统，观测性能指标，并为异常情况设置告警。

无论你是要将 Load Ants 用于企业内部 DNS 解析、搭建高可用的 DNS 服务，还是用于个人项目，本章都提供了对应的部署方案。

---

## 下一步

- [➡️ 使用 Docker Compose 部署](./docker-compose.md)
- [➡️ 部署为系统服务](./system-service.md)
- [➡️ 在 Kubernetes 中部署](./kubernetes.md)
- [➡️ 了解安全注意事项](./security.md)
- [➡️ 了解如何监控服务](./monitoring.md)
