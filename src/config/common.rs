use crate::r#const::retry_limits;
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

// 认证类型枚举
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    // HTTP基本认证
    Basic,
    // Bearer令牌认证
    Bearer,
}

// 自定义验证函数 - 验证Basic认证配置
fn validate_basic_auth(auth: &AuthConfig) -> Result<(), ValidationError> {
    if matches!(auth.r#type, AuthType::Basic) {
        if auth.username.is_none() || auth.username.as_ref().unwrap().is_empty() {
            return Err(ValidationError::new("missing_username_for_basic_auth"));
        }
        if auth.password.is_none() || auth.password.as_ref().unwrap().is_empty() {
            return Err(ValidationError::new("missing_password_for_basic_auth"));
        }
    }
    Ok(())
}

// 自定义验证函数 - 验证Bearer认证配置
fn validate_bearer_auth(auth: &AuthConfig) -> Result<(), ValidationError> {
    if matches!(auth.r#type, AuthType::Bearer) && (auth.token.is_none() || auth.token.as_ref().unwrap().is_empty()) {
        return Err(ValidationError::new("missing_token_for_bearer_auth"));
    }
    Ok(())
}

// 认证配置
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[validate(schema(
    function = "validate_basic_auth",
    message = "Basic authentication requires username and password"
))]
#[validate(schema(
    function = "validate_bearer_auth",
    message = "Bearer authentication requires token"
))]
#[serde(rename_all = "lowercase")]
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
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Validate)]
#[serde(rename_all = "lowercase")]
pub struct RetryConfig {
    // 重试次数
    #[validate(range(
        min = retry_limits::MIN_ATTEMPTS,
        max = retry_limits::MAX_ATTEMPTS,
        message = "Retry attempts must be between {} and {}"
    ))]
    pub attempts: u32,
    // 重试初始延迟（秒）
    #[validate(range(
        min = retry_limits::MIN_DELAY,
        max = retry_limits::MAX_DELAY,
        message = "Retry delay must be between {} and {} seconds"
    ))]
    pub delay: u32,
}
