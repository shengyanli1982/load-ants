// 声明子模块
mod doh;
mod http_client;
mod json;
mod manager;

// 重导出公共API，保持与原来相同的接口
pub use manager::UpstreamManager;

// 增强重导出
pub use doh::DoHClient;
pub use http_client::HttpClient;
