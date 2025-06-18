// src/doh/mod.rs
//
// DoH (DNS over HTTPS) 服务器模块实现，支持:
// - RFC 8484: 标准 DoH 协议，支持 GET 和 POST 方法
// - Google JSON API: Google 格式的 DoH API，仅支持 GET 方法

// 子模块定义
pub mod handlers;
pub mod json;
pub mod server;
pub mod state;

// 公开导出
pub use server::DoHServer;
