use crate::config::UpstreamServerConfig;
use crate::error::AppError;
use crate::metrics::METRICS;
use rand::{seq::SliceRandom, thread_rng};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU8, AtomicUsize, Ordering};
use tracing::{info, warn};

use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_MAX_FAILURES: u32 = 3;
pub const DEFAULT_COOLDOWN_SECS: i64 = 30;

pub trait LoadBalancer: Send + Sync {
    fn servers(&self) -> &[UpstreamServerConfig];
    fn health_states(&self) -> &[ServerHealth];
    fn select_server(&self, exclude: &[usize]) -> Result<&UpstreamServerConfig, AppError>;

    fn report_failure(&self, server: &UpstreamServerConfig, _group_name: &str) {
        if let Some(idx) = self.servers().iter().position(|s| s == server) {
            self.health_states()[idx].record_failure();
            self.health_states()[idx].check_and_log_transition();
        }
    }

    fn report_success(&self, server: &UpstreamServerConfig, _group_name: &str) {
        if let Some(idx) = self.servers().iter().position(|s| s == server) {
            self.health_states()[idx].record_success();
            self.health_states()[idx].check_and_log_transition();
        }
    }

    fn server_count(&self) -> usize {
        self.servers().len()
    }
}

fn server_identifier(server: &UpstreamServerConfig) -> String {
    match server {
        UpstreamServerConfig::Doh(s) => s.url.as_str().to_string(),
        UpstreamServerConfig::Dns(s) => s.addr.to_string(),
    }
}

pub(crate) enum ServerState {
    Healthy,
    Unhealthy,
    HalfOpen,
}

impl ServerState {
    fn as_u8(&self) -> u8 {
        match self {
            ServerState::Unhealthy => 0,
            ServerState::HalfOpen => 1,
            ServerState::Healthy => 2,
        }
    }
}

fn state_label(state_v: u8) -> &'static str {
    match state_v {
        0 => "unhealthy",
        1 => "half_open",
        2 => "healthy",
        _ => "unknown",
    }
}

pub struct ServerHealth {
    failure_count: AtomicU32,
    last_failure_time: AtomicI64,
    half_open_probing: AtomicBool,
    last_observed_state: AtomicU8,
    max_failures: u32,
    cooldown_secs: i64,
    group_label: String,
    server_label: String,
}

impl ServerHealth {
    fn new(max_failures: u32, cooldown_secs: i64, group_label: &str, server_label: &str) -> Self {
        Self {
            failure_count: AtomicU32::new(0),
            last_failure_time: AtomicI64::new(0),
            half_open_probing: AtomicBool::new(false),
            last_observed_state: AtomicU8::new(ServerState::Healthy.as_u8()),
            max_failures,
            cooldown_secs,
            group_label: group_label.to_string(),
            server_label: server_label.to_string(),
        }
    }

    pub(crate) fn check_and_log_transition(&self) {
        self.observe_state_change(self.state().as_u8());
    }

    fn observe_state_change(&self, new_state_v: u8) {
        let old_state_v = self.last_observed_state.load(Ordering::Acquire);
        if old_state_v == new_state_v {
            return;
        }
        if self
            .last_observed_state
            .compare_exchange(
                old_state_v,
                new_state_v,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return;
        }

        let state_from = state_label(old_state_v);
        let state_to = state_label(new_state_v);

        METRICS
            .circuit_breaker_transitions_total
            .with_label_values(&[&self.group_label, state_from, state_to])
            .inc();

        METRICS
            .upstream_health_state
            .with_label_values(&[&self.group_label, &self.server_label])
            .set(new_state_v as i64);

        match (old_state_v, new_state_v) {
            (2, 0) | (1, 0) => {
                warn!(
                    group = %self.group_label,
                    server = %self.server_label,
                    from = %state_from,
                    to = %state_to,
                    "Circuit breaker tripped"
                );
            }
            _ => {
                info!(
                    group = %self.group_label,
                    server = %self.server_label,
                    from = %state_from,
                    to = %state_to,
                    "Circuit breaker transition"
                );
            }
        }
    }

    pub fn failure_count(&self) -> u32 {
        self.failure_count.load(Ordering::Acquire)
    }

    pub(crate) fn state(&self) -> ServerState {
        let failures = self.failure_count.load(Ordering::Acquire);
        if failures < self.max_failures {
            return ServerState::Healthy;
        }
        if self.half_open_probing.load(Ordering::Acquire) {
            return ServerState::Unhealthy;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let last_failure = self.last_failure_time.load(Ordering::Acquire);
        if now - last_failure >= self.cooldown_secs {
            self.observe_state_change(ServerState::HalfOpen.as_u8());
            ServerState::HalfOpen
        } else {
            ServerState::Unhealthy
        }
    }

    fn try_claim_probe(&self) -> bool {
        self.half_open_probing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub(crate) fn record_failure(&self) {
        self.failure_count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
                Some(v.saturating_add(1))
            })
            .ok();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.last_failure_time.store(now, Ordering::Release);
        self.half_open_probing.store(false, Ordering::Release);
    }

    pub(crate) fn record_success(&self) {
        self.failure_count.store(0, Ordering::Release);
        self.half_open_probing.store(false, Ordering::Release);
    }
}

const STACK_INDEX_CAPACITY: usize = 64;

struct IndexBuffer {
    stack: [usize; STACK_INDEX_CAPACITY],
    len: usize,
    overflow: Option<Vec<usize>>,
}

impl IndexBuffer {
    fn new() -> Self {
        Self {
            stack: [0; STACK_INDEX_CAPACITY],
            len: 0,
            overflow: None,
        }
    }

    fn push(&mut self, index: usize) {
        if let Some(indices) = &mut self.overflow {
            indices.push(index);
            return;
        }
        if self.len < STACK_INDEX_CAPACITY {
            self.stack[self.len] = index;
            self.len += 1;
            return;
        }
        let mut indices = Vec::with_capacity(STACK_INDEX_CAPACITY * 2);
        indices.extend_from_slice(&self.stack);
        indices.push(index);
        self.overflow = Some(indices);
    }

    fn as_slice(&self) -> &[usize] {
        match &self.overflow {
            Some(indices) => indices.as_slice(),
            None => &self.stack[..self.len],
        }
    }
}

fn compute_available_indices(health_states: &[ServerHealth]) -> IndexBuffer {
    let mut healthy = IndexBuffer::new();
    let mut half_open = IndexBuffer::new();

    for (i, health) in health_states.iter().enumerate() {
        match health.state() {
            ServerState::Healthy => healthy.push(i),
            ServerState::HalfOpen => half_open.push(i),
            ServerState::Unhealthy => {}
        }
    }

    if healthy.as_slice().is_empty() {
        half_open
    } else {
        healthy
    }
}

fn filter_excluded(available: &[usize], exclude: &[usize]) -> IndexBuffer {
    let mut candidates = IndexBuffer::new();
    for index in available {
        if !exclude.contains(index) {
            candidates.push(*index);
        }
    }
    candidates
}

pub struct RoundRobinBalancer {
    servers: Vec<UpstreamServerConfig>,
    current: AtomicUsize,
    health_states: Vec<ServerHealth>,
}

impl RoundRobinBalancer {
    pub fn new(group_name: &str, servers: Vec<UpstreamServerConfig>) -> Self {
        Self::with_params(
            group_name,
            servers,
            DEFAULT_MAX_FAILURES,
            DEFAULT_COOLDOWN_SECS,
        )
    }

    pub fn with_params(
        group_name: &str,
        servers: Vec<UpstreamServerConfig>,
        max_failures: u32,
        cooldown_secs: i64,
    ) -> Self {
        let health_states = servers
            .iter()
            .map(|server| {
                ServerHealth::new(
                    max_failures,
                    cooldown_secs,
                    group_name,
                    &server_identifier(server),
                )
            })
            .collect();
        Self {
            servers,
            current: AtomicUsize::new(0),
            health_states,
        }
    }
}

impl LoadBalancer for RoundRobinBalancer {
    fn servers(&self) -> &[UpstreamServerConfig] {
        &self.servers
    }

    fn health_states(&self) -> &[ServerHealth] {
        &self.health_states
    }

    fn select_server(&self, exclude: &[usize]) -> Result<&UpstreamServerConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        let idx = self
            .pick_index(exclude)
            .ok_or(AppError::NoUpstreamAvailable)?;
        if matches!(self.health_states[idx].state(), ServerState::HalfOpen) {
            self.health_states[idx].try_claim_probe();
        }
        Ok(&self.servers[idx])
    }
}

impl RoundRobinBalancer {
    fn pick_index(&self, exclude: &[usize]) -> Option<usize> {
        let n = self.servers.len();
        if n <= STACK_INDEX_CAPACITY {
            let healthy_code = ServerState::Healthy.as_u8();
            let half_open_code = ServerState::HalfOpen.as_u8();
            let mut tiers = [0u8; STACK_INDEX_CAPACITY];
            let mut healthy = 0usize;
            let mut half_open = 0usize;
            for (i, health) in self.health_states.iter().enumerate() {
                let code = health.state().as_u8();
                tiers[i] = code;
                if exclude.contains(&i) {
                    continue;
                }
                if code == healthy_code {
                    healthy += 1;
                } else if code == half_open_code {
                    half_open += 1;
                }
            }
            let (tier, total) = if healthy > 0 {
                (healthy_code, healthy)
            } else {
                (half_open_code, half_open)
            };
            if total == 0 {
                return None;
            }
            let offset = self.current.fetch_add(1, Ordering::AcqRel) % total;
            return (0..n)
                .filter(|i| tiers[*i] == tier && !exclude.contains(i))
                .nth(offset);
        }

        let available = compute_available_indices(&self.health_states);
        let candidates = filter_excluded(available.as_slice(), exclude);
        let candidates = candidates.as_slice();
        if candidates.is_empty() {
            return None;
        }
        Some(candidates[self.current.fetch_add(1, Ordering::AcqRel) % candidates.len()])
    }
}

pub struct WeightedBalancer {
    servers: Vec<UpstreamServerConfig>,
    current_weights: Vec<AtomicI64>,
    health_states: Vec<ServerHealth>,
}

impl WeightedBalancer {
    pub fn new(group_name: &str, servers: Vec<UpstreamServerConfig>) -> Self {
        Self::with_params(
            group_name,
            servers,
            DEFAULT_MAX_FAILURES,
            DEFAULT_COOLDOWN_SECS,
        )
    }

    pub fn with_params(
        group_name: &str,
        servers: Vec<UpstreamServerConfig>,
        max_failures: u32,
        cooldown_secs: i64,
    ) -> Self {
        let current_weights = (0..servers.len()).map(|_| AtomicI64::new(0)).collect();
        let health_states = servers
            .iter()
            .map(|server| {
                ServerHealth::new(
                    max_failures,
                    cooldown_secs,
                    group_name,
                    &server_identifier(server),
                )
            })
            .collect();

        Self {
            servers,
            current_weights,
            health_states,
        }
    }
}

impl LoadBalancer for WeightedBalancer {
    fn servers(&self) -> &[UpstreamServerConfig] {
        &self.servers
    }

    fn health_states(&self) -> &[ServerHealth] {
        &self.health_states
    }

    fn select_server(&self, exclude: &[usize]) -> Result<&UpstreamServerConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        let available = compute_available_indices(&self.health_states);
        let candidates = filter_excluded(available.as_slice(), exclude);
        let candidates = candidates.as_slice();
        if candidates.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        let is_subset = candidates.len() < self.servers.len();
        if is_subset {
            for i in 0..self.servers.len() {
                if !candidates.contains(&i) {
                    self.current_weights[i].store(0, Ordering::Relaxed);
                }
            }
        }

        let available_total_weight: i64 = candidates
            .iter()
            .map(|i| self.servers[*i].weight() as i64)
            .fold(0, |acc, w| acc.saturating_add(w));

        for i in candidates {
            let weight = self.servers[*i].weight() as i64;
            self.current_weights[*i]
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    Some(current.saturating_add(weight))
                })
                .ok();
        }

        let mut max_weight = i64::MIN;
        let mut max_index = candidates[0];
        for i in candidates {
            let w = self.current_weights[*i].load(Ordering::Relaxed);
            if w > max_weight {
                max_weight = w;
                max_index = *i;
            }
        }

        self.current_weights[max_index]
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(available_total_weight))
            })
            .ok();

        if matches!(self.health_states[max_index].state(), ServerState::HalfOpen) {
            self.health_states[max_index].try_claim_probe();
        }

        Ok(&self.servers[max_index])
    }
}

pub struct RandomBalancer {
    servers: Vec<UpstreamServerConfig>,
    health_states: Vec<ServerHealth>,
}

impl RandomBalancer {
    pub fn new(group_name: &str, servers: Vec<UpstreamServerConfig>) -> Self {
        Self::with_params(
            group_name,
            servers,
            DEFAULT_MAX_FAILURES,
            DEFAULT_COOLDOWN_SECS,
        )
    }

    pub fn with_params(
        group_name: &str,
        servers: Vec<UpstreamServerConfig>,
        max_failures: u32,
        cooldown_secs: i64,
    ) -> Self {
        let health_states = servers
            .iter()
            .map(|server| {
                ServerHealth::new(
                    max_failures,
                    cooldown_secs,
                    group_name,
                    &server_identifier(server),
                )
            })
            .collect();
        Self {
            servers,
            health_states,
        }
    }
}

impl LoadBalancer for RandomBalancer {
    fn servers(&self) -> &[UpstreamServerConfig] {
        &self.servers
    }

    fn health_states(&self) -> &[ServerHealth] {
        &self.health_states
    }

    fn select_server(&self, exclude: &[usize]) -> Result<&UpstreamServerConfig, AppError> {
        if self.servers.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        let available = compute_available_indices(&self.health_states);
        let candidates = filter_excluded(available.as_slice(), exclude);
        let candidates = candidates.as_slice();
        if candidates.is_empty() {
            return Err(AppError::NoUpstreamAvailable);
        }

        let idx = candidates
            .choose(&mut thread_rng())
            .copied()
            .ok_or(AppError::NoUpstreamAvailable)?;
        if matches!(self.health_states[idx].state(), ServerState::HalfOpen) {
            self.health_states[idx].try_claim_probe();
        }
        Ok(&self.servers[idx])
    }
}
