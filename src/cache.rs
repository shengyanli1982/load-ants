use crate::error::AppError;
use crate::metrics::METRICS;
use crate::r#const::{cache_labels, cache_limits, ttl_source_labels};
use hickory_proto::op::{Message, ResponseCode};
use hickory_proto::rr::{DNSClass, RecordType};
use moka::future::Cache;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::time::Duration;
use tracing::{debug, info};

// DNS缓存键
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    // 域名
    name: String,
    // 记录类型
    record_type: RecordType,
    // DNS类
    class: DNSClass,
}

impl CacheKey {
    // 从DNS查询消息创建缓存键
    fn from_message(message: &Message) -> Option<Self> {
        let query = message.queries().first()?;

        Some(Self {
            name: query.name().to_string(),
            record_type: query.query_type(),
            class: query.query_class(),
        })
    }
}

// DNS缓存条目
#[derive(Debug, Clone)]
struct CacheEntry {
    // 缓存的响应消息
    message: Message,
    // 缓存时间戳
    timestamp: std::time::Instant,
    // 缓存时长 (秒)
    _ttl: u32,
}

// DNS缓存
pub struct DnsCache {
    // 缓存存储
    cache: Cache<CacheKey, CacheEntry>,
    // 缓存大小限制
    size: usize,
    // 最小TTL (秒)
    min_ttl: u32,
    // 负面缓存TTL (秒)
    negative_ttl: u32,
}

impl DnsCache {
    // 创建新的DNS缓存
    pub fn new(size: usize, min_ttl: u32, negative_ttl: Option<u32>) -> Self {
        // 验证配置
        let size = size.clamp(cache_limits::MIN_SIZE, cache_limits::MAX_SIZE);
        let min_ttl = min_ttl.clamp(cache_limits::MIN_TTL, cache_limits::MAX_TTL);
        // 使用配置的负面缓存TTL或默认值
        let negative_ttl = negative_ttl
            .unwrap_or(cache_limits::DEFAULT_NEGATIVE_TTL)
            .clamp(cache_limits::MIN_TTL, cache_limits::MAX_TTL);

        // 创建缓存
        let cache = Cache::builder()
            .max_capacity(size as u64)
            // 过期时间为最大可能的TTL
            .time_to_live(Duration::from_secs(cache_limits::MAX_TTL as u64))
            .build();

        info!(
            "Creating DNS cache - Size: {}, Min TTL: {}s, Negative TTL: {}s",
            size, min_ttl, negative_ttl
        );

        // 设置缓存容量指标
        METRICS.cache_capacity().set(size as i64);

        Self {
            cache,
            size,
            min_ttl,
            negative_ttl,
        }
    }

    // 检查缓存是否启用
    pub fn is_enabled(&self) -> bool {
        self.size > 0
    }

    // 从缓存中获取响应
    pub async fn get(&self, query: &Message) -> Option<Message> {
        // 创建缓存键
        let key = CacheKey::from_message(query)?;

        // 从缓存中查找
        let entry = self.cache.get(&key).await?;

        // 克隆响应
        let mut response = entry.message.clone();

        // 调整TTL
        self.adjust_message_ttl(&mut response, &entry);

        // 如果是 A 或 AAAA 记录查询，对答案进行随机排序
        if key.record_type == RecordType::A || key.record_type == RecordType::AAAA {
            self.shuffle_message_records(&mut response, key.record_type);
        }

        // 更新缓存命中指标
        METRICS
            .cache_operations_total()
            .with_label_values(&[cache_labels::HIT])
            .inc();

        Some(response)
    }

    // 对 DNS 中指定类型的记录进行随机排序
    fn shuffle_message_records(&self, message: &mut Message, record_type: RecordType) {
        // 获取所有答案记录
        let answers = message.answers().to_vec();

        // 如果记录数量小于2，无需排序
        if answers.len() < 2 {
            return;
        }

        // 按记录类型分组
        let mut target_records = Vec::new();
        let mut other_records = Vec::new();

        // 只对目标类型的记录进行分组
        for record in answers {
            if record.record_type() == record_type {
                target_records.push(record);
            } else {
                other_records.push(record);
            }
        }

        // 如果目标类型的记录少于2个，无需随机排序
        if target_records.len() < 2 {
            return;
        }

        // 只对目标类型的记录进行随机排序
        target_records.shuffle(&mut thread_rng());

        // 清除原有答案并按顺序重新添加
        message.take_answers();

        // 使用 add_answers 批量添加记录，提高性能
        message.add_answers(target_records);
        message.add_answers(other_records);
    }

    // 向缓存添加响应
    pub async fn insert(&self, query: &Message, response: Message) -> Result<(), AppError> {
        // 检查是否可缓存
        if !self.is_cacheable(&response) {
            debug!("Response not cacheable");
            return Ok(());
        }

        // 创建缓存键
        let key = match CacheKey::from_message(query) {
            Some(k) => k,
            None => {
                debug!("Cannot create cache key from query");
                // 增加插入错误指标
                METRICS
                    .cache_operations_total()
                    .with_label_values(&[cache_labels::INSERT_ERROR])
                    .inc();
                return Err(AppError::Cache(
                    "Cannot create cache key from query".to_string(),
                ));
            }
        };

        // 计算TTL
        let ttl = self.calculate_min_ttl(&response);

        // 记录TTL指标
        METRICS
            .cache_ttl_seconds()
            .with_label_values(&[ttl_source_labels::ADJUSTED])
            .observe(ttl as f64);

        // 创建缓存条目
        let entry = CacheEntry {
            message: response,
            timestamp: std::time::Instant::now(),
            _ttl: ttl,
        };

        // 插入缓存
        self.cache.insert(key.clone(), entry).await;
        debug!("Added to cache - {} ({:?})", key.name, key.record_type);

        // 更新缓存指标
        METRICS
            .cache_operations_total()
            .with_label_values(&[cache_labels::INSERT])
            .inc();
        METRICS.cache_entries().set(self.cache.entry_count() as i64);

        Ok(())
    }

    // 清空缓存
    #[allow(dead_code)]
    pub async fn clear(&self) {
        debug!("Clearing DNS cache");
        self.cache.invalidate_all();

        // 更新缓存指标
        METRICS
            .cache_operations_total()
            .with_label_values(&[cache_labels::CLEAR])
            .inc();
        METRICS.cache_entries().set(0);
    }

    // 检查响应是否可缓存
    fn is_cacheable(&self, response: &Message) -> bool {
        // 所有响应都可以缓存，无论是成功响应还是错误响应
        // 但仍然需要确保查询部分存在
        if response.queries().is_empty() {
            debug!("Response contains no query, not caching");
            return false;
        }

        true
    }

    // 计算响应中所有记录的最小TTL
    fn calculate_min_ttl(&self, response: &Message) -> u32 {
        // 对于错误响应或没有答案的响应，使用负面缓存TTL
        if response.response_code() != ResponseCode::NoError || response.answer_count() == 0 {
            // 记录使用负面缓存TTL指标
            METRICS
                .cache_ttl_seconds()
                .with_label_values(&[ttl_source_labels::NEGATIVE_TTL])
                .observe(self.negative_ttl as f64);

            debug!(
                "Using negative cache TTL ({} seconds) for response code: {:?}",
                self.negative_ttl,
                response.response_code()
            );

            return self.negative_ttl;
        }

        let mut min_ttl = u32::MAX;

        // 检查所有回答记录的TTL
        for record in response.answers() {
            min_ttl = min_ttl.min(record.ttl());
        }

        // 如果找到有效的TTL，记录原始值
        if min_ttl != u32::MAX {
            // 记录原始TTL指标
            METRICS
                .cache_ttl_seconds()
                .with_label_values(&[ttl_source_labels::ORIGINAL])
                .observe(min_ttl as f64);
        }

        // 如果没有找到有效的TTL，使用最小TTL
        if min_ttl == u32::MAX {
            min_ttl = self.min_ttl;

            // 记录使用最小TTL指标
            METRICS
                .cache_ttl_seconds()
                .with_label_values(&[ttl_source_labels::MIN_TTL])
                .observe(min_ttl as f64);
        } else {
            // 应用最小TTL限制
            let before_adjustment = min_ttl;
            min_ttl = min_ttl.max(self.min_ttl);

            // 如果TTL被调整了，记录指标
            if before_adjustment != min_ttl {
                METRICS
                    .cache_ttl_seconds()
                    .with_label_values(&[ttl_source_labels::MIN_TTL])
                    .observe(min_ttl as f64);
            }
        }

        // 记录TTL调整
        METRICS
            .cache_operations_total()
            .with_label_values(&[cache_labels::ADJUSTED])
            .inc();

        min_ttl
    }

    // 调整响应消息中的TTL
    fn adjust_message_ttl(&self, message: &mut Message, entry: &CacheEntry) {
        // 计算经过的时间 (秒)
        let elapsed_secs = entry.timestamp.elapsed().as_secs() as u32;

        // 调整答案记录的TTL
        for record in message.answers_mut() {
            let original_ttl = record.ttl();
            let new_ttl = if original_ttl > elapsed_secs {
                original_ttl - elapsed_secs
            } else {
                1 // 至少保留1秒TTL
            };
            record.set_ttl(new_ttl);

            // 记录实际TTL指标（经过时间调整后）
            METRICS
                .cache_ttl_seconds()
                .with_label_values(&[ttl_source_labels::ADJUSTED])
                .observe(new_ttl as f64);
        }

        // 调整权威记录的TTL
        for record in message.name_servers_mut() {
            let original_ttl = record.ttl();
            let new_ttl = if original_ttl > elapsed_secs {
                original_ttl - elapsed_secs
            } else {
                1 // 至少保留1秒TTL
            };
            record.set_ttl(new_ttl);
        }

        // 调整附加记录的TTL
        for record in message.additionals_mut() {
            // 跳过OPT记录
            if record.record_type() == RecordType::OPT {
                continue;
            }

            let original_ttl = record.ttl();
            let new_ttl = if original_ttl > elapsed_secs {
                original_ttl - elapsed_secs
            } else {
                1 // 至少保留1秒TTL
            };
            record.set_ttl(new_ttl);
        }

        // 记录TTL调整
        METRICS
            .cache_operations_total()
            .with_label_values(&[cache_labels::ADJUSTED])
            .inc();
    }

    // 获取缓存条目数量
    pub async fn len(&self) -> usize {
        self.cache.run_pending_tasks().await;
        self.cache.entry_count() as usize
    }

    // 检查缓存是否为空
    #[allow(dead_code)]
    pub async fn is_empty(&self) -> bool {
        self.cache.run_pending_tasks().await;
        self.cache.entry_count() == 0
    }
}
