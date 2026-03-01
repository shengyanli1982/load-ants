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
    about = r#"
 __         ______     ______     _____     ______     __   __     ______   ______    
/\ \       /\  __ \   /\  __ \   /\  __-.  /\  __ \   /\ "-.\ \   /\__  _\ /\  ___\   
\ \ \____  \ \ \/\ \  \ \  __ \  \ \ \/\ \ \ \  __ \  \ \ \-.  \  \/_/\ \/ \ \___  \  
 \ \_____\  \ \_____\  \ \_\ \_\  \ \____-  \ \_\ \_\  \ \_\\"\_\    \ \_\  \/\_____\ 
  \/_____/   \/_____/   \/_/\/_/   \/____/   \/_/\/_/   \/_/ \/_/     \/_/   \/_____/ 
          

A lightweight DNS splitter & forwarder.

Key Features:
- Multi-protocol Inbound: UDP/53, TCP/53, and DoH (RFC 8484)
- Multiple Upstream Schemes: DoH and classic DNS (UDP/TCP) via `upstream_groups[].scheme` (doh|dns)
- Classic DNS Client: UDP by default, fallback to TCP when UDP response is truncated (TC=1), or `prefer_tcp=true`
- Intelligent Routing: exact / wildcard / regex
- Load Balancing: round-robin / weighted / random
- Performance: built-in positive/negative cache, reusable connection pools
- Observability: Prometheus metrics (`/metrics` on admin server)
- Usability: YAML config, startup validation, `--test` mode

Docs: https://shengyanli1982.github.io/load-ants/

Author: shengyanli1982
Email: shengyanlee36@gmail.com
GitHub: https://github.com/shengyanli1982/load-ants
"#
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
