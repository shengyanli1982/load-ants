use crate::config::UpstreamServerConfig;
use crate::error::AppError;
use async_trait::async_trait;
use rand::{seq::SliceRandom, thread_rng};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

// 负载均衡器特性
#[async_trait]
pub trait LoadBalancer: Send + Sync {
    // 选择一个上游服务器
    async fn select_server(&self) -> Result<&UpstreamServerConfig, AppError>;

    // 报告服务器失败
    async fn report_failure(&self, server: &UpstreamServerConfig);
}

// 轮询负载均衡器
pub struct RoundRobinBalancer {
    // 服务器列表
    servers: Vec<UpstreamServerConfig>,
    // 当前索引（原子操作）
    current: AtomicUsize,
}

impl RoundRobinBalancer {
    // 创建新的轮询负载均衡器
    pub fn new(servers: Vec<UpstreamServerConfig>) -> Self {
        Self {
            servers,
            current: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LoadBalancer for RoundRobinBalancer {
    async fn select_server(&self) -> Result<&UpstreamServerConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        let current = self.current.fetch_add(1, Ordering::SeqCst) % self.servers.len();
        Ok(&self.servers[current])
    }

    async fn report_failure(&self, _server: &UpstreamServerConfig) {
        // 轮询策略下不需要特殊处理失败
    }
}

// 加权轮询负载均衡器
pub struct WeightedBalancer {
    // 服务器列表
    servers: Vec<UpstreamServerConfig>,
    // 当前权重（Mutex 保护以确保并发安全）
    current_weights: Mutex<Vec<i64>>,
    // 总权重
    total_weight: i64,
}

impl WeightedBalancer {
    // 创建新的加权轮询负载均衡器
    pub fn new(servers: Vec<UpstreamServerConfig>) -> Self {
        // 计算权重总和
        let total_weight = servers.iter().map(|s| s.weight() as i64).sum();

        // 初始化当前权重为0
        let current_weights = servers.iter().map(|_| 0i64).collect();

        Self {
            servers,
            current_weights: Mutex::new(current_weights),
            total_weight,
        }
    }
}

#[async_trait]
impl LoadBalancer for WeightedBalancer {
    async fn select_server(&self) -> Result<&UpstreamServerConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        let mut weights = self.current_weights.lock().unwrap();

        // 平滑加权轮询算法实现
        let mut max_weight = i64::MIN;
        let mut max_index = 0;

        // 第一步：为每个服务器增加当前权重并选择最大的
        for (i, current_weight) in weights.iter_mut().enumerate() {
            // 增加当前权重
            let effective_weight = self.servers[i].weight() as i64;
            *current_weight += effective_weight;

            // 查找当前最大权重的服务器
            if *current_weight > max_weight {
                max_weight = *current_weight;
                max_index = i;
            }
        }

        // 第二步：减少选中服务器的当前权重
        weights[max_index] -= self.total_weight;

        // 返回选中的服务器
        Ok(&self.servers[max_index])
    }

    async fn report_failure(&self, _server: &UpstreamServerConfig) {
        // 加权轮询策略下不需要特殊处理失败
    }
}

// 随机负载均衡器
pub struct RandomBalancer {
    // 服务器列表
    servers: Vec<UpstreamServerConfig>,
}

impl RandomBalancer {
    // 创建新的随机负载均衡器
    pub fn new(servers: Vec<UpstreamServerConfig>) -> Self {
        Self { servers }
    }
}

#[async_trait]
impl LoadBalancer for RandomBalancer {
    async fn select_server(&self) -> Result<&UpstreamServerConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        let server = self
            .servers
            .choose(&mut thread_rng())
            .ok_or(AppError::NoUpstreamAvailable)?;
        Ok(server)
    }

    async fn report_failure(&self, _server: &UpstreamServerConfig) {
        // 随机策略下不需要特殊处理失败
    }
}
