use crate::api::state::AppState;
use axum::{extract::State, Json};
use serde::Serialize;

#[derive(Serialize)]
pub struct MetricsDto {
    pub total_calls: u64,
    pub success_rate: f64,
    pub avg_latency_ms: f64,
    pub total_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub host: HostMetricsDto,
    pub engram: benshu_engram::HybridSearchStats,
}

#[derive(Serialize)]
pub struct HostMetricsDto {
    pub cpu_usage_percent: f32,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub active_agent_processes: usize,
    pub os_name: String,
    pub uptime_secs: u64,
    pub disk_usage_percent: f32,
    pub net_rx_kbps: f32,
    pub net_tx_kbps: f32,
    pub gpu_vram_used_mb: u32,
    pub gpu_vram_total_mb: u32,
    pub gpu_utilization_percent: f32,
    pub suggested_quantization: String,
}

pub async fn metrics_handler(State(state): State<AppState>) -> Json<MetricsDto> {
    let snapshot = state.kernel.coordinator().metrics.get_snapshot();

    let mut total_calls = 0;
    let mut total_errors = 0;
    let mut total_latencies_sum = 0.0;
    let mut total_latencies_count = 0;
    let mut total_tokens = 0;
    let mut prompt_tokens = 0;
    let mut completion_tokens = 0;

    for (name, val) in snapshot {
        if name.ends_with(":tool_calls_total") {
            if let benshu_brain::infra::observable::MetricValue::Counter(c) = val {
                total_calls += c;
            }
        } else if name.ends_with(":tool_errors_total") {
            if let benshu_brain::infra::observable::MetricValue::Counter(c) = val {
                total_errors += c;
            }
        } else if name.ends_with(":tool_duration_ms") {
            if let benshu_brain::infra::observable::MetricValue::Histogram { count, sum, .. } = val
            {
                total_latencies_sum += sum;
                total_latencies_count += count;
            }
        } else if name.ends_with(":tokens_total") {
            if let benshu_brain::infra::observable::MetricValue::Counter(c) = val {
                total_tokens += c;
            }
        } else if name.ends_with(":tokens_prompt_total") {
            if let benshu_brain::infra::observable::MetricValue::Counter(c) = val {
                prompt_tokens += c;
            }
        } else if name.ends_with(":tokens_completion_total") {
            if let benshu_brain::infra::observable::MetricValue::Counter(c) = val {
                completion_tokens += c;
            }
        }
    }

    let success_rate = if total_calls > 0 {
        (total_calls as f64 - total_errors as f64) / total_calls as f64
    } else {
        1.0
    };

    let avg_latency_ms = if total_latencies_count > 0 {
        total_latencies_sum / total_latencies_count as f64
    } else {
        0.0
    };

    Json(MetricsDto {
        total_calls,
        success_rate,
        avg_latency_ms,
        total_tokens,
        prompt_tokens,
        completion_tokens,
        host: get_host_metrics(),
        engram: state.kernel.search_engine().stats(),
    })
}

pub fn get_host_metrics() -> HostMetricsDto {
    use sysinfo::{CpuRefreshKind, Disks, Networks, RefreshKind, System};
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(sysinfo::MemoryRefreshKind::everything()),
    );
    sys.refresh_all();

    let cpu_usage = sys.global_cpu_usage();
    let memory_used = sys.used_memory() / (1024 * 1024);
    let memory_total = sys.total_memory() / (1024 * 1024);
    let active_agents = benshu_security::sandbox::ACTIVE_SANDBOXES.len();

    let os_name = System::name().unwrap_or_else(|| "Unknown".to_string());
    let uptime = System::uptime();

    let disks = Disks::new_with_refreshed_list();
    let mut total_space = 0;
    let mut total_available = 0;
    for disk in &disks {
        total_space += disk.total_space();
        total_available += disk.available_space();
    }
    let disk_usage = if total_space > 0 {
        ((total_space - total_available) as f32 / total_space as f32) * 100.0
    } else {
        0.0
    };

    static LAST_NET: parking_lot::Mutex<Option<(std::time::Instant, u64, u64)>> =
        parking_lot::Mutex::new(None);
    let networks = Networks::new_with_refreshed_list();
    let mut current_rx = 0;
    let mut current_tx = 0;
    for (_name, net) in &networks {
        current_rx += net.received();
        current_tx += net.transmitted();
    }

    let now = std::time::Instant::now();
    let mut last_net_lock = LAST_NET.lock();
    let (net_rx_kbps, net_tx_kbps) = if let Some((last_time, last_rx, last_tx)) = *last_net_lock {
        let elapsed = now.duration_since(last_time).as_secs_f32();
        if elapsed > 0.0 {
            let rx = (current_rx.saturating_sub(last_rx) as f32 / 1024.0) / elapsed;
            let tx = (current_tx.saturating_sub(last_tx) as f32 / 1024.0) / elapsed;
            (rx, tx)
        } else {
            (0.0, 0.0)
        }
    } else {
        (0.0, 0.0)
    };
    *last_net_lock = Some((now, current_rx, current_tx));

    HostMetricsDto {
        cpu_usage_percent: cpu_usage,
        memory_used_mb: memory_used,
        memory_total_mb: memory_total,
        active_agent_processes: active_agents,
        os_name,
        uptime_secs: uptime,
        disk_usage_percent: disk_usage,
        net_rx_kbps,
        net_tx_kbps,
        gpu_vram_used_mb: 0, // Placeholder for GPU detection
        gpu_vram_total_mb: 0,
        gpu_utilization_percent: 0.0,
        suggested_quantization: "Q4_K_M".into(),
    }
}
