// 声明子模块
mod doh;
mod http_client;
mod json;
mod manager;

// 重导出公共API，保持与原来相同的接口
pub use manager::UpstreamManager;

// 不需要对外暴露内部类型
// pub(crate) use json::JsonConverter;
// pub(crate) use doh::DoHClient;
// pub(crate) use http_client::HttpClient;
