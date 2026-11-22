use aeris_core::TimestampNs;
use aeris_execution::ExchangeOrderGateway;
use aeris_gd::{
    GdEscalationState, GuardianSentry, HeartbeatPacket, DEFAULT_GD_PORT,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;

fn current_time_ns() -> TimestampNs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

#[tokio::main]
async fn main() {
    println!("\n================================================================================");
    println!("             AERIS GD (GUARDIAN DAEMON - FAIL-SAFE SENTRY RUNTIME)              ");
    println!("================================================================================\n");

    let bind_addr = format!("127.0.0.1:{}", DEFAULT_GD_PORT);
    let socket = match UdpSocket::bind(&bind_addr).await {
        Ok(s) => {
            println!("[GD INIT] Bound UDP fail-safe sentry listener to {}", bind_addr);
            Arc::new(s)
        }
        Err(e) => {
            eprintln!("[GD FATAL] Failed to bind GD socket to {}: {}", bind_addr, e);
            std::process::exit(1);
        }
    };

    let lockfile = PathBuf::from("D:/Pushp - Do not open without permission/AERIS/AERIS_EMERGENCY_LOCKOUT");
    let gateway = ExchangeOrderGateway::auto_from_env(true, true); // testnet / dry-run safe fallback
    let sentry = Arc::new(tokio::sync::Mutex::new(GuardianSentry::new(
        500,   // 500ms soft warning
        1000,  // 1000ms hard cancel-all
        3000,  // 3000ms nuclear market flatten
        lockfile,
        Some(gateway),
    )));

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!("\n[GD SHUTDOWN] Signal received. Shutting down Guardian Daemon cleanly.");
        r.store(false, Ordering::SeqCst);
    });

    // Receiver task
    let s_sock = socket.clone();
    let s_sentry = sentry.clone();
    let r_run = running.clone();

    tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        while r_run.load(Ordering::SeqCst) {
            match s_sock.recv_from(&mut buf).await {
                Ok((len, _peer)) => {
                    if let Ok(packet) = serde_json::from_slice::<HeartbeatPacket>(&buf[..len]) {
                        let now_ns = current_time_ns();
                        let mut guard = s_sentry.lock().await;
                        let _ = guard.process_heartbeat(&packet, now_ns);
                    }
                }
                Err(e) => {
                    eprintln!("[GD WARN] Error receiving UDP packet: {}", e);
                }
            }
        }
    });

    println!("[GD READY] Sentry armed. Monitoring aeris-node heartbeats (Escalation: 500ms -> 1000ms -> 3000ms)...");

    let mut tick_interval = tokio::time::interval(Duration::from_millis(100));
    let mut last_display = std::time::Instant::now();

    while running.load(Ordering::SeqCst) {
        tick_interval.tick().await;

        let now_ns = current_time_ns();
        let mut guard = sentry.lock().await;
        let state = guard.evaluate_watchdog(now_ns);

        if last_display.elapsed() >= Duration::from_millis(2000) {
            println!(
                "[GD STATUS] State: {:?} | Last Seq: {} | Last Seen: {:.2}s ago",
                state,
                guard.last_seq,
                if guard.last_heartbeat_ns == 0 {
                    0.0
                } else {
                    (now_ns.saturating_sub(guard.last_heartbeat_ns) as f64) / 1_000_000_000.0
                }
            );
            last_display = std::time::Instant::now();
        }

        if state == GdEscalationState::NuclearFlattened {
            eprintln!("[GD HALT] Nuclear flatten triggered! Halting watchdog loop to preserve evidence.");
            break;
        }
    }

    println!("[GD EXIT] Guardian Daemon terminated cleanly.");
}
