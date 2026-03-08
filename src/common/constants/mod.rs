mod defaults;
mod labels;
mod limits;
mod misc;

pub use defaults::{server_defaults, upstream_defaults};

pub use labels::{
    cache_labels, error_labels, processing_labels, protocol_labels, rule_action_labels,
    rule_source_labels, rule_type_labels, subsystem_names, ttl_source_labels, upstream_labels,
    upstream_protocol_labels, upstream_transport_labels,
};

pub use limits::{
    bootstrap_dns_limits, cache_limits, dns_client_limits, http_client_limits, remote_rule_limits,
    retry_limits, shutdown_timeout, timeout_limits, weight_limits,
};

pub use misc::{http_headers, router};
