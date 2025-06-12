mod loader;
mod parser;

pub use self::loader::RemoteRuleLoader;
pub use self::parser::{ClashRuleParser, RuleParser, V2RayRuleParser};

use crate::config::{HttpClientConfig, RemoteRuleConfig, RouteRuleConfig};
use crate::error::AppError;
use tracing::error;

/// 加载所有远程规则并与本地规则合并
pub async fn load_and_merge_rules(
    remote_configs: &[RemoteRuleConfig],
    static_rules: &[RouteRuleConfig],
    http_config: &HttpClientConfig,
) -> Result<Vec<RouteRuleConfig>, AppError> {
    // 创建一个规则列表，预先分配足够的空间
    let mut merged_rules = Vec::with_capacity(static_rules.len() + remote_configs.len() * 3);

    // 首先添加静态规则（通过克隆）
    merged_rules.extend_from_slice(static_rules);

    // 加载每个远程规则
    for config in remote_configs {
        match RemoteRuleLoader::new(config.clone(), http_config.clone()) {
            Ok(loader) => {
                match loader.load().await {
                    Ok(remote_rules) => {
                        // 将远程规则添加到合并规则列表，避免不必要的克隆
                        merged_rules.extend(remote_rules);
                    }
                    Err(e) => {
                        // 记录错误但继续处理其他规则
                        error!("Failed to load domains from {:?}: {}", config.url, e);
                    }
                }
            }
            Err(e) => {
                // 记录错误但继续处理其他规则
                error!(
                    "Failed to create remote rule loader for {}: {}",
                    config.url, e
                );
            }
        }
    }

    Ok(merged_rules)
}
