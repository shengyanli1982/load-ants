use hickory_proto::ProtoError;
use loadants::server::{dns_server_result, effective_udp_socket_count};
use loadants::AppError;

/// D-17: `ServerFuture::block_until_done` 的运行期错误必须向上传播为 AppError，
/// 使 toplevel 收到子系统失败 → main.rs 以非零退出码结束；
/// 不得吞掉错误上报 Ok(())（否则 systemd/docker 重启策略与监控无法感知异常退出）。
#[test]
fn test_dns_server_result_propagates_error() {
    let err = ProtoError::from("simulated server future failure");
    let result = dns_server_result(Err(err));
    assert!(
        matches!(result, Err(AppError::DnsProto(_))),
        "block_until_done error must propagate as Err(AppError::DnsProto(_)), got {result:?}"
    );
}

/// D-17 回归护栏: 正常完成仍返回 Ok(())。
#[test]
fn test_dns_server_result_passes_through_ok() {
    assert!(dns_server_result(Ok(())).is_ok());
}

/// D-18: socket 数量的平台无关下限归一（0 → 1）。
#[test]
fn test_socket_count_zero_normalizes_to_one() {
    assert_eq!(effective_udp_socket_count(0), 1);
    assert_eq!(effective_udp_socket_count(1), 1);
}

/// D-18: Windows 下 udp_socket_count > 1 必须降级为单 socket——
/// Windows 无 SO_REUSEPORT，多 socket 同端口绑定只能依赖 SO_REUSEADDR，
/// 而后者允许本机其他进程劫持端口（见下方 windows_socket_empirics 实测证据）。
#[test]
#[cfg(windows)]
fn test_windows_clamps_multi_socket_to_one() {
    assert_eq!(effective_udp_socket_count(4), 1);
    assert_eq!(effective_udp_socket_count(256), 1);
}

/// D-18: unix 平台保持多 socket 配置不变（SO_REUSEPORT 可用，现状行为保留）。
#[test]
#[cfg(unix)]
fn test_unix_preserves_multi_socket() {
    assert_eq!(effective_udp_socket_count(4), 4);
}

/// D-18 实测证据（仅 Windows）: 验证 SO_REUSEADDR 双绑定行为，
/// 确认审计判断"Windows 下 SO_REUSEADDR 允许端口被其他进程劫持"是否成立。
#[cfg(windows)]
mod windows_socket_empirics {
    use socket2::{Domain, Protocol, Socket, Type};
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn udp_socket() -> Socket {
        Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).unwrap()
    }

    /// 实测①: 两个 socket 均设 SO_REUSEADDR → 同一端口双绑定成功，
    /// 即任何本机进程都可用 SO_REUSEADDR 劫持已绑定的端口（审计判断成立的证据）。
    #[test]
    fn so_reuseaddr_allows_double_bind() {
        let first = udp_socket();
        first.set_reuse_address(true).unwrap();
        first
            .bind(&SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0).into())
            .unwrap();
        let port = first
            .local_addr()
            .unwrap()
            .as_socket()
            .expect("bound socket has address")
            .port();

        let second = udp_socket();
        second.set_reuse_address(true).unwrap();
        let result = second.bind(&SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into());
        assert!(
            result.is_ok(),
            "实测预期: Windows 下 SO_REUSEADDR 允许双绑定(端口可被劫持), 实际: {result:?}"
        );
    }

    /// 实测②: 不设 SO_REUSEADDR → Windows 默认 bind 排他，双绑定失败。
    /// 这是"Windows 平台移除 reuse 设置即可恢复排他语义"修复方案的依据。
    #[test]
    fn default_bind_is_exclusive() {
        let first = udp_socket();
        first
            .bind(&SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0).into())
            .unwrap();
        let port = first
            .local_addr()
            .unwrap()
            .as_socket()
            .expect("bound socket has address")
            .port();

        let second = udp_socket();
        let result = second.bind(&SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into());
        assert!(
            result.is_err(),
            "实测预期: Windows 默认 bind 排他(不设 SO_REUSEADDR 时双绑定失败), 实际: {result:?}"
        );
    }
}
