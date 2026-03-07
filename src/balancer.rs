use crate::config::{HealthConfig, UpstreamEndpointConfig};
use crate::error::AppError;
use async_trait::async_trait;
use rand::{seq::SliceRandom, thread_rng};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tokio::time::{Duration, Instant};

// 负载均衡器特性
#[async_trait]
pub trait LoadBalancer: Send + Sync {
    // 选择一个上游服务器
    async fn select_server(&self) -> Result<&UpstreamEndpointConfig, AppError>;

    // 报告服务器失败
    async fn report_failure(&self, server: &UpstreamEndpointConfig);

    // 报告服务器成功
    async fn report_success(&self, server: &UpstreamEndpointConfig);
}

#[derive(Debug, Clone, Copy, Default)]
struct EndpointHealthState {
    failures: u32,
    cooldown_until: Option<Instant>,
}

struct BalancerHealth {
    cfg: HealthConfig,
    states: Mutex<Vec<EndpointHealthState>>,
}

impl BalancerHealth {
    fn new(cfg: HealthConfig, servers_len: usize) -> Self {
        Self {
            cfg,
            states: Mutex::new(vec![EndpointHealthState::default(); servers_len]),
        }
    }

    fn is_available(&self, index: usize, now: Instant) -> bool {
        let states = self.states.lock().unwrap();
        let Some(state) = states.get(index) else {
            return false;
        };

        match state.cooldown_until {
            Some(until) => now >= until,
            None => true,
        }
    }

    fn report_failure(&self, index: usize, now: Instant) {
        let mut states = self.states.lock().unwrap();
        let Some(state) = states.get_mut(index) else {
            return;
        };

        state.failures = state.failures.saturating_add(1);
        if state.failures >= self.cfg.failure_threshold {
            state.failures = 0;
            state.cooldown_until = Some(now + Duration::from_secs(self.cfg.cooldown_seconds));
        }
    }

    fn report_success(&self, index: usize) {
        if !self.cfg.success_reset {
            return;
        }

        let mut states = self.states.lock().unwrap();
        let Some(state) = states.get_mut(index) else {
            return;
        };

        state.failures = 0;
        state.cooldown_until = None;
    }
}

// 轮询负载均衡器
pub struct RoundRobinBalancer {
    // 服务器列表
    servers: Vec<UpstreamEndpointConfig>,
    // 当前索引（原子操作）
    current: AtomicUsize,
    // 健康状态（可选）
    health: Option<BalancerHealth>,
}

impl RoundRobinBalancer {
    // 创建新的轮询负载均衡器
    pub fn new(servers: Vec<UpstreamEndpointConfig>, health: Option<HealthConfig>) -> Self {
        let health = health.map(|cfg| BalancerHealth::new(cfg, servers.len()));
        Self {
            servers,
            current: AtomicUsize::new(0),
            health,
        }
    }

    fn server_index(&self, server: &UpstreamEndpointConfig) -> Option<usize> {
        self.servers.iter().position(|s| s == server)
    }
}

#[async_trait]
impl LoadBalancer for RoundRobinBalancer {
    async fn select_server(&self) -> Result<&UpstreamEndpointConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        if let Some(health) = &self.health {
            let now = Instant::now();
            for _ in 0..self.servers.len() {
                let current = self.current.fetch_add(1, Ordering::SeqCst) % self.servers.len();
                if health.is_available(current, now) {
                    return Ok(&self.servers[current]);
                }
            }
            return Err(AppError::NoUpstreamAvailable);
        }

        let current = self.current.fetch_add(1, Ordering::SeqCst) % self.servers.len();
        Ok(&self.servers[current])
    }

    async fn report_failure(&self, server: &UpstreamEndpointConfig) {
        let Some(health) = &self.health else {
            return;
        };
        let Some(index) = self.server_index(server) else {
            return;
        };
        health.report_failure(index, Instant::now());
    }

    async fn report_success(&self, server: &UpstreamEndpointConfig) {
        let Some(health) = &self.health else {
            return;
        };
        let Some(index) = self.server_index(server) else {
            return;
        };
        health.report_success(index);
    }
}

// 加权轮询负载均衡器
pub struct WeightedBalancer {
    // 服务器列表
    servers: Vec<UpstreamEndpointConfig>,
    // 当前权重（原子操作）
    current_weights: Vec<AtomicUsize>,
    // 健康状态（可选）
    health: Option<BalancerHealth>,
}

impl WeightedBalancer {
    // 创建新的加权轮询负载均衡器
    pub fn new(servers: Vec<UpstreamEndpointConfig>, health: Option<HealthConfig>) -> Self {
        // 初始化当前权重为0
        let current_weights = servers.iter().map(|_| AtomicUsize::new(0)).collect();
        let health = health.map(|cfg| BalancerHealth::new(cfg, servers.len()));

        Self {
            servers,
            current_weights,
            health,
        }
    }

    fn server_index(&self, server: &UpstreamEndpointConfig) -> Option<usize> {
        self.servers.iter().position(|s| s == server)
    }
}

#[async_trait]
impl LoadBalancer for WeightedBalancer {
    async fn select_server(&self) -> Result<&UpstreamEndpointConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        if let Some(health) = &self.health {
            let now = Instant::now();
            let mut available_total_weight: usize = 0;
            for (i, server) in self.servers.iter().enumerate() {
                if health.is_available(i, now) {
                    available_total_weight =
                        available_total_weight.saturating_add(server.weight() as usize);
                }
            }
            if available_total_weight == 0 {
                return Err(AppError::NoUpstreamAvailable);
            }

            let mut max_weight = 0;
            let mut max_index = None;

            for (i, weight_atomic) in self.current_weights.iter().enumerate() {
                if !health.is_available(i, now) {
                    continue;
                }

                let weight = self.servers[i].weight() as usize;
                let current = weight_atomic.fetch_add(weight, Ordering::SeqCst) + weight;
                if max_index.is_none() || current > max_weight {
                    max_weight = current;
                    max_index = Some(i);
                }
            }

            let Some(max_index) = max_index else {
                return Err(AppError::NoUpstreamAvailable);
            };

            self.current_weights[max_index].fetch_sub(available_total_weight, Ordering::SeqCst);
            return Ok(&self.servers[max_index]);
        }

        // 平滑加权轮询算法实现
        let mut max_weight = 0;
        let mut max_index = 0;

        // 第一步：为每个服务器增加当前权重并选择最大的
        for (i, weight_atomic) in self.current_weights.iter().enumerate() {
            // 增加当前权重
            let weight = self.servers[i].weight() as usize;
            let current = weight_atomic.fetch_add(weight, Ordering::SeqCst) + weight;

            // 查找当前最大权重的服务器
            if current > max_weight {
                max_weight = current;
                max_index = i;
            }
        }

        // 第二步：减少选中服务器的当前权重
        let total_weight: usize = self.servers.iter().map(|s| s.weight() as usize).sum();
        self.current_weights[max_index].fetch_sub(total_weight, Ordering::SeqCst);

        // 返回选中的服务器
        Ok(&self.servers[max_index])
    }

    async fn report_failure(&self, server: &UpstreamEndpointConfig) {
        let Some(health) = &self.health else {
            return;
        };
        let Some(index) = self.server_index(server) else {
            return;
        };
        health.report_failure(index, Instant::now());
    }

    async fn report_success(&self, server: &UpstreamEndpointConfig) {
        let Some(health) = &self.health else {
            return;
        };
        let Some(index) = self.server_index(server) else {
            return;
        };
        health.report_success(index);
    }
}

// 随机负载均衡器
pub struct RandomBalancer {
    // 服务器列表
    servers: Vec<UpstreamEndpointConfig>,
    // 健康状态（可选）
    health: Option<BalancerHealth>,
}

impl RandomBalancer {
    // 创建新的随机负载均衡器
    pub fn new(servers: Vec<UpstreamEndpointConfig>, health: Option<HealthConfig>) -> Self {
        let health = health.map(|cfg| BalancerHealth::new(cfg, servers.len()));
        Self { servers, health }
    }

    fn server_index(&self, server: &UpstreamEndpointConfig) -> Option<usize> {
        self.servers.iter().position(|s| s == server)
    }
}

#[async_trait]
impl LoadBalancer for RandomBalancer {
    async fn select_server(&self) -> Result<&UpstreamEndpointConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        if let Some(health) = &self.health {
            let now = Instant::now();
            let available_indices: Vec<usize> = (0..self.servers.len())
                .filter(|i| health.is_available(*i, now))
                .collect();

            let index = available_indices
                .choose(&mut thread_rng())
                .copied()
                .ok_or(AppError::NoUpstreamAvailable)?;
            return Ok(&self.servers[index]);
        }

        let server = self
            .servers
            .choose(&mut thread_rng())
            .ok_or(AppError::NoUpstreamAvailable)?;
        Ok(server)
    }

    async fn report_failure(&self, server: &UpstreamEndpointConfig) {
        let Some(health) = &self.health else {
            return;
        };
        let Some(index) = self.server_index(server) else {
            return;
        };
        health.report_failure(index, Instant::now());
    }

    async fn report_success(&self, server: &UpstreamEndpointConfig) {
        let Some(health) = &self.health else {
            return;
        };
        let Some(index) = self.server_index(server) else {
            return;
        };
        health.report_success(index);
    }
}
