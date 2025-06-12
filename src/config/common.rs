use serde::{Deserialize, Serialize};

// 认证类型枚举
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    // HTTP基本认证
    Basic,
    // Bearer令牌认证
    Bearer,
}

// 认证配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct AuthConfig {
    // 认证类型（basic/bearer）
    pub r#type: AuthType,
    // 用户名（仅用于basic认证）
    pub username: Option<String>,
    // 密码（仅用于basic认证）
    pub password: Option<String>,
    // 令牌（仅用于bearer认证）
    pub token: Option<String>,
}

// 重试配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct RetryConfig {
    // 重试次数
    pub attempts: u32,
    // 重试初始延迟（秒）
    pub delay: u32,
}
