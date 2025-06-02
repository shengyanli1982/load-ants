use crate::error::AppError;
use crate::r#const::shutdown_timeout;
use clap::{ArgAction, Parser};
use std::path::PathBuf;

// DNS UDP/TCP to DoH 代理服务
#[derive(Parser, Debug, Clone)]
#[command(
    name = "loadants",
    author,
    version,
    about = "An lightweight DNS forwarder converting UDP/TCP queries to DoH\n\n\
             Key Features:\n\
             - Protocol Conversion: UDP/53 and TCP/53 DNS requests to DNS-over-HTTPS (RFC 8484)\n\
             - Intelligent Routing: Exact domain matching, Wildcard domain matching, Regex pattern matching\n\
             - Upstream Management: Server grouping, Multiple load balancing strategies (RR, WRR, Random)\n\
             - Authentication Support: HTTP Basic Auth and Bearer Token for upstream DoH servers\n\
             - Performance Optimization: Built-in DNS caching, Reusable HTTP connection pool\n\
             - Reliability: Automatic retry mechanism for failed DoH requests\n\
             - Usability: Simple YAML configuration, Configuration validation, Command-line interface\n\n\
             Author: shengyanli1982\n\
             Email: shengyanlee36@gmail.com\n\
             GitHub: https://github.com/shengyanli1982"
)]
pub struct Args {
    // 配置文件路径
    #[arg(short, long, default_value = "./config.yaml")]
    pub config: PathBuf,

    // 测试配置
    #[arg(
        short = 't', 
        long = "test", 
        action = ArgAction::SetTrue, 
        help = "Test configuration file for validity and exit"
    )]
    pub test_config: bool,

    // 启用调试日志
    #[arg(
        short = 'd',
        long = "debug",
        action = ArgAction::SetTrue,
        help = "Enable debug level logging for detailed output"
    )]
    pub debug: bool,

    // 关闭超时
    #[arg(
        long = "shutdown-timeout",
        help = "Maximum time in seconds to wait for complete shutdown",
        default_value_t = shutdown_timeout::DEFAULT
    )]
    pub shutdown_timeout: u64,
}

impl Args {
    // 解析命令行参数
    pub fn parse_args() -> Self {
        Args::parse()
    }

    // 验证参数
    pub fn validation(&self) -> Result<(), AppError> {
        if self.shutdown_timeout < shutdown_timeout::MIN
            || self.shutdown_timeout > shutdown_timeout::MAX
        {
            return Err(AppError::InvalidShutdownTimeout);
        }
        Ok(())
    }
}
