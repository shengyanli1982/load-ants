use clap::Parser;
use std::path::PathBuf;

// DNS UDP/TCP to DoH 代理服务
#[derive(Parser, Debug, Clone)]
#[command(
    name = "load-ants",
    author,
    version,
    about = "高性能DNS代理，将UDP/TCP DNS请求转换为DoH"
)]
pub struct Args {
    // 配置文件路径
    #[arg(short, long, default_value = "./config.yaml")]
    pub config: PathBuf,

    // 测试配置文件有效性
    #[arg(short, long, default_value_t = false)]
    pub test: bool,
}

impl Args {
    // 解析命令行参数
    pub fn parse_args() -> Self {
        Args::parse()
    }
}
