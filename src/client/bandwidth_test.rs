use log::{debug, info, warn};
use reqwest;
use std::time::{Duration, Instant};

/// Lightweight heartbeat check - just verifies SOCKS connectivity
/// Uses Cloudflare's CDN trace endpoint (very lightweight, ~300 bytes response)
/// TODO figure out a way to bandwidth test the websites you are already visiting
pub async fn heartbeat_check(socks_port: u16) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    heartbeat_with_endpoints(socks_port, &[
        "https://cloudflare.com/cdn-cgi/trace", "https://example.com/"
    ]).await
}

async fn heartbeat_with_endpoints(socks_port: u16, endpoints: &[&str])
    -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let proxy = reqwest::Proxy::all(format!("socks5h://127.0.0.1:{}", socks_port))?;
    let client = reqwest::Client::builder().proxy(proxy)
        .timeout(Duration::from_secs(10)).build()?;
    let mut last_error = None;
    for endpoint in endpoints {
        match client.get(*endpoint).send().await {
            Ok(response) => {
                // A site-specific 403/429/5xx is still evidence of working
                // end-to-end transport; do not tear down paid circuits for it.
                debug!("SOCKS heartbeat received HTTP {}", response.status());
                return Ok(true);
            }
            Err(error) => last_error = Some(error),
        }
    }
    if let Some(error) = last_error { return Err(error.into()); }
    Ok(false)
}

/// Full bandwidth test - downloads a file to test throughput
/// Uses Cloudflare's speed test file (reliable global CDN)
pub async fn bandwidth_test(socks_port: u16) -> Result<(u64, f64), Box<dyn std::error::Error + Send + Sync>> {
    let start = Instant::now();
    
    // Create SOCKS5 proxy URL with 'h' suffix for DNS resolution through proxy
    let proxy_url = format!("socks5h://127.0.0.1:{}", socks_port);
    let proxy = reqwest::Proxy::all(&proxy_url)?;
    
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(45))
        .build()?;
    
    // Cloudflare's speed test endpoint - 3MB file for better throughput measurement
    // Larger file gives more accurate bandwidth measurement
    // TODO make this configurable, is there a reliable way to do this thru Tor's RPC?
    let response = client
        .get("https://speed.cloudflare.com/__down?bytes=3000000")
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(format!("Bandwidth test failed: HTTP {}", response.status()).into());
    }
    
    let headers_time = start.elapsed(); // Time to get response headers
    
    // Download the content - measure actual data transfer speed
    let download_start = Instant::now();
    let content = response.bytes().await?;
    let download_duration = download_start.elapsed();
    
    let test_size_bytes = content.len();
    let total_duration = start.elapsed();
    
    // Calculate speeds
    // Transfer speed: just the download phase (more accurate for bandwidth)
    let download_secs = download_duration.as_secs_f64();
    let transfer_speed_kbps = if download_secs > 0.0 {
        (test_size_bytes as f64 / 1024.0) / download_secs
    } else {
        0.0
    };
    let transfer_speed_mbps = transfer_speed_kbps / 1024.0;
    
    // Overall speed: includes connection setup (what user experiences)
    let total_secs = total_duration.as_secs_f64();
    let overall_speed_kbps = (test_size_bytes as f64 / 1024.0) / total_secs;
    let overall_speed_mbps = overall_speed_kbps / 1024.0;
    
    let latency_ms = headers_time.as_millis() as u64;
    let total_ms = total_duration.as_millis() as u64;
    
    info!(
        "📊 Bandwidth: {:.1} KB/s ({:.2} MB/s) transfer, {:.1} KB/s ({:.2} MB/s) overall | Latency: {}ms | Total: {}ms for {:.1} MB",
        transfer_speed_kbps, transfer_speed_mbps, overall_speed_kbps, overall_speed_mbps, 
        latency_ms, total_ms, test_size_bytes as f64 / 1_000_000.0
    );
    
    Ok((total_ms, transfer_speed_kbps))
}

/// Real bandwidth check using SOCKS proxy test with retry
/// This is the most reliable way to verify the circuit is actually routing traffic
/// Retries up to 3 times with exponential backoff (1s, 2s, 4s) to handle timing issues
pub async fn has_bandwidth(socks_port: u16) -> bool {
    has_bandwidth_with_retry(socks_port, 2).await
}

/// Internal function with configurable retry count
async fn has_bandwidth_with_retry(socks_port: u16, max_retries: u32) -> bool {
    let mut retries = 0;
    
    loop {
        match heartbeat_check(socks_port).await {
            Ok(true) => {
                if retries > 0 {
                    info!("✅ SOCKS bandwidth check passed (after {} retries)", retries);
                } else {
                    debug!("✅ SOCKS bandwidth check passed");
                }
                return true;
            }
            Ok(false) => {
                warn!("❌ SOCKS bandwidth check failed: Heartbeat returned false");
                return false;
            }
            Err(e) => {
                retries += 1;
                if retries >= max_retries {
                    warn!("❌ SOCKS bandwidth check failed after {} retries: {}", max_retries, e);
                    return false;
                }
                
                // Exponential backoff: 1s, 2s, 4s
                let delay_secs = 2_u64.pow(retries - 1);
                warn!(
                    "⚠️  SOCKS check failed (attempt {}/{}): {}. Retrying in {}s...",
                    retries, max_retries, e, delay_secs
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
            }
        }
    }
}

/// Checks if we need to build more circuits based on stream count.
/// Returns (total_streams, needs_more_circuits)
pub async fn check_stream_capacity(rpc_config: &crate::types::RpcConfig) -> (usize, bool) {
    // Get stream status
    let stream_status_config = crate::types::RpcConfig {
        addr: rpc_config.addr.clone(),
        rpc_password: rpc_config.rpc_password.clone(),
        command: "GETINFO stream-status".to_string(),
    };
    
    let stream_response = match crate::rpc::rpc_client(stream_status_config).await {
        Ok(resp) => resp,
        Err(e) => {
            warn!("Failed to get stream status: {}", e);
            return (0, false);
        }
    };
    
    // Count active streams
    let active_streams = stream_response.lines()
        .filter(|line| {
            !line.starts_with("250") && line.trim() != "." && !line.is_empty()
        })
        .filter(|line| {
            line.contains(" SUCCEEDED ")
        })
        .count();
    
    // Get circuit count
    let circuit_status_config = crate::types::RpcConfig {
        addr: rpc_config.addr.clone(),
        rpc_password: rpc_config.rpc_password.clone(),
        command: "GETINFO circuit-status".to_string(),
    };
    
    let circuit_response = match crate::rpc::rpc_client(circuit_status_config).await {
        Ok(resp) => resp,
        Err(_) => return (active_streams, false),
    };
    
    let built_circuits = circuit_response.lines()
        .filter(|line| {
            !line.starts_with("250") && line.trim() != "." && !line.is_empty()
        })
        .filter(|line| {
            line.contains(" BUILT ")
        })
        .count();
    
    // Calculate if we need more circuits
    // Rule: If average streams per circuit > 200 (80% of 256 limit), build more
    let needs_more = if built_circuits > 0 {
        let avg_streams_per_circuit = active_streams / built_circuits;
        avg_streams_per_circuit > 200
    } else {
        false
    };
    
    if needs_more {
        info!(
            "⚠️  Stream capacity warning: {} streams across {} circuits (avg: {} per circuit)",
            active_streams, built_circuits, active_streams / built_circuits
        );
        info!("🔄 Recommendation: Build more circuits to avoid hitting 256-stream limit");
    }
    
    (active_streams, needs_more)
}

#[cfg(test)]
mod stability_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt,AsyncReadExt,AsyncWriteExt,BufReader};

    async fn proxy(fail_first: bool) -> (u16, tokio::task::JoinHandle<()>) {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port=listener.local_addr().unwrap().port();
        let handle=tokio::spawn(async move {
            for attempt in 0..if fail_first {2} else {1} {
                let (mut stream,_)=listener.accept().await.unwrap();
                let mut greeting=[0;3];stream.read_exact(&mut greeting).await.unwrap();
                stream.write_all(&[5,0]).await.unwrap();
                let mut header=[0;5];stream.read_exact(&mut header).await.unwrap();
                let mut rest=vec![0;header[4] as usize+2];stream.read_exact(&mut rest).await.unwrap();
                if fail_first && attempt==0 {
                    stream.write_all(&[5,1,0,1,0,0,0,0,0,0]).await.unwrap();
                    continue;
                }
                stream.write_all(&[5,0,0,1,0,0,0,0,0,0]).await.unwrap();
                let mut reader=BufReader::new(stream);
                loop {
                    let mut line=String::new();reader.read_line(&mut line).await.unwrap();
                    if line=="\r\n" {break;}
                }
                reader.get_mut().write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            }
        });
        (port,handle)
    }

    #[tokio::test]
    async fn website_status_is_not_circuit_failure() {
        let (port,server)=proxy(false).await;
        assert!(heartbeat_with_endpoints(port,&["http://first.invalid/"]).await.unwrap());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn retries_an_independent_endpoint_after_transport_failure() {
        let (port,server)=proxy(true).await;
        assert!(heartbeat_with_endpoints(port,&["http://first.invalid/","http://second.invalid/"]).await.unwrap());
        server.await.unwrap();
    }
}
