use crate::balancer::{LoadBalancer, RandomBalancer, RoundRobinBalancer, WeightedBalancer};
use crate::config::{HttpClientConfig, LoadBalancingStrategy, UpstreamGroupConfig};
use crate::error::AppError;
use crate::metrics::METRICS;
use crate::r#const::{error_labels, upstream_labels};
use crate::upstream::doh::DoHClient;
use crate::upstream::http_client::HttpClient;
use hickory_proto::op::Message;
use reqwest_middleware::ClientWithMiddleware;
use std::{collections::HashMap, sync::Arc};
use tracing::{debug, error, info};

// 上游管理器
pub struct UpstreamManager {
    // 上游组负载均衡器
    groups: HashMap<String, Arc<dyn LoadBalancer>>,
    // 上游组客户端
    group_clients: HashMap<String, ClientWithMiddleware>,
}

impl UpstreamManager {
    // 创建新的上游管理器
    pub async fn new(
        groups: Vec<UpstreamGroupConfig>,
        http_config: HttpClientConfig,
    ) -> Result<Self, AppError> {
        let mut group_map = HashMap::with_capacity(groups.len());
        let mut group_clients = HashMap::new();

        // 为每个组创建负载均衡器和HTTP客户端
        for UpstreamGroupConfig {
            name,
            strategy,
            servers,
            retry,
            proxy,
        } in groups
        {
            let lb: Arc<dyn LoadBalancer> = match strategy {
                LoadBalancingStrategy::RoundRobin => Arc::new(RoundRobinBalancer::new(servers)),
                LoadBalancingStrategy::Weighted => Arc::new(WeightedBalancer::new(servers)),
                LoadBalancingStrategy::Random => Arc::new(RandomBalancer::new(servers)),
            };

            // 创建该组的HTTP客户端
            let client = HttpClient::create(&http_config, proxy.as_deref(), retry.as_ref())?;
            group_clients.insert(name.clone(), client);

            group_map.insert(name, lb);
        }

        info!("Initialized {} upstream groups", group_map.len());

        Ok(Self {
            groups: group_map,
            group_clients,
        })
    }

    // 转发查询到指定上游组
    pub async fn forward(&self, query: &Message, group_name: &str) -> Result<Message, AppError> {
        debug!("Forwarding request to upstream group: {}", group_name);

        // 获取上游组的负载均衡器
        let load_balancer = match self.groups.get(group_name) {
            Some(lb) => lb,
            None => {
                error!("Upstream group not found: {}", group_name);
                return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
            }
        };

        // 选择一个上游服务器
        let server = match load_balancer.select_server().await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to select upstream server: {}", e);

                // 记录上游错误指标
                METRICS
                    .upstream_errors_total()
                    .with_label_values(&[
                        error_labels::SELECT_ERROR,
                        group_name,
                        upstream_labels::UNKNOWN,
                    ])
                    .inc();

                return Err(e);
            }
        };

        debug!("Selected upstream server: {}", server.url.as_str());

        // 记录上游请求指标
        METRICS
            .upstream_requests_total()
            .with_label_values(&[group_name, server.url.as_str()])
            .inc();

        // 记录开始时间
        let start_time = std::time::Instant::now();

        // 获取组的HTTP客户端
        let client = match self.group_clients.get(group_name) {
            Some(c) => c,
            None => {
                error!("HTTP client not found for group: {}", group_name);
                return Err(AppError::UpstreamGroupNotFound(group_name.to_string()));
            }
        };

        // 发送请求（通过reqwest-retry中间件处理重试）
        let doh_client = DoHClient::new(client);
        match doh_client.send_request(query, server).await {
            Ok(response) => {
                // 记录上游请求耗时
                let duration = start_time.elapsed();
                METRICS
                    .upstream_duration_seconds()
                    .with_label_values(&[group_name, server.url.as_str()])
                    .observe(duration.as_secs_f64());

                Ok(response)
            }
            Err(e) => {
                error!("Upstream request failed: {} - {}", server.url.as_str(), e);

                // 报告上游失败
                load_balancer.report_failure(server).await;

                // 记录上游错误指标
                METRICS
                    .upstream_errors_total()
                    .with_label_values(&[
                        error_labels::REQUEST_ERROR,
                        group_name,
                        server.url.as_str(),
                    ])
                    .inc();

                Err(e)
            }
        }
    }
}
