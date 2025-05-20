use crate::config::UpstreamServerConfig;
use crate::error::AppError;
use async_trait::async_trait;
use rand::{seq::SliceRandom, thread_rng};
use std::sync::atomic::{AtomicUsize, Ordering};

// 负载均衡器特性
#[async_trait]
pub trait LoadBalancer: Send + Sync {
    // 选择一个上游服务器
    async fn select_server(&self) -> Result<UpstreamServerConfig, AppError>;

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
    async fn select_server(&self) -> Result<UpstreamServerConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        let current = self.current.fetch_add(1, Ordering::SeqCst) % self.servers.len();
        Ok(self.servers[current].clone())
    }

    async fn report_failure(&self, _server: &UpstreamServerConfig) {
        // 轮询策略下不需要特殊处理失败
    }
}

// 加权轮询负载均衡器
pub struct WeightedBalancer {
    // 服务器列表
    servers: Vec<UpstreamServerConfig>,
    // 当前权重（原子操作）
    current_weights: Vec<AtomicUsize>,
    // 总权重
    total_weight: usize,
}

impl WeightedBalancer {
    // 创建新的加权轮询负载均衡器
    pub fn new(servers: Vec<UpstreamServerConfig>) -> Self {
        // 计算权重总和
        let total_weight = servers.iter().map(|s| s.weight as usize).sum();

        // 初始化当前权重为0
        let current_weights = servers.iter().map(|_| AtomicUsize::new(0)).collect();

        Self {
            servers,
            current_weights,
            total_weight,
        }
    }
}

#[async_trait]
impl LoadBalancer for WeightedBalancer {
    async fn select_server(&self) -> Result<UpstreamServerConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        // 平滑加权轮询算法实现
        let mut max_weight = 0;
        let mut max_index = 0;

        // 第一步：为每个服务器增加当前权重并选择最大的
        for (i, weight_atomic) in self.current_weights.iter().enumerate() {
            // 增加当前权重
            let weight = self.servers[i].weight as usize;
            let current = weight_atomic.fetch_add(weight, Ordering::SeqCst) + weight;

            // 查找当前最大权重的服务器
            if current > max_weight {
                max_weight = current;
                max_index = i;
            }
        }

        // 第二步：减少选中服务器的当前权重
        self.current_weights[max_index].fetch_sub(self.total_weight, Ordering::SeqCst);

        // 返回选中的服务器
        Ok(self.servers[max_index].clone())
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
    async fn select_server(&self) -> Result<UpstreamServerConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        let server = self
            .servers
            .choose(&mut thread_rng())
            .ok_or(AppError::NoUpstreamAvailable)?;
        Ok(server.clone())
    }

    async fn report_failure(&self, _server: &UpstreamServerConfig) {
        // 随机策略下不需要特殊处理失败
    }
}
