//! Regression tests for intermittent circuit and control failures.
use eltor::{rpc, types::RpcConfig};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    time::Duration,
};

fn fake_control(
    reject: bool,
    disconnect_subscription: bool,
) -> (RpcConfig, Arc<Mutex<Vec<String>>>) {
    fake_control_with_events(reject, disconnect_subscription, "")
}

fn fake_control_with_events(
    reject: bool,
    disconnect_subscription: bool,
    events: &'static str,
) -> (RpcConfig, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let commands = Arc::new(Mutex::new(Vec::new()));
    let recorded = commands.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let recorded = recorded.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let command = line.trim().to_owned();
                    recorded.lock().unwrap().push(command.clone());
                    let reply = if command.starts_with("AUTHENTICATE") {
                        "250 OK\r\n"
                    } else if command == "QUIT" {
                        "250 closing connection\r\n"
                    } else if command == "GETCONF PaymentCircuitMaxFee" {
                        "250 PaymentCircuitMaxFee=1000\r\n"
                    } else if command == "GETCONF PaymentLightningNodeConfig" {
                        "250-PaymentLightningNodeConfig=type=wrong default=false\r\n250 PaymentLightningNodeConfig=type=phoenixd url=http://localhost password=test-only default=true\r\n"
                    } else if command.starts_with("ATTACHSTREAM 42 ") {
                        "555 Connection is not managed by controller.\r\n"
                    } else if reject {
                        "552 Deliberately rejected for diagnostic test\r\n"
                    } else {
                        "250 OK\r\n"
                    };
                    if stream.write_all(reply.as_bytes()).is_err() {
                        break;
                    }
                    if command == "QUIT" {
                        break;
                    }
                    if command == "SETEVENTS STREAM" {
                        let _ = stream.write_all(events.as_bytes());
                    }
                    if command == "SETEVENTS STREAM" && disconnect_subscription {
                        break;
                    }
                }
            });
        }
    });
    (
        RpcConfig {
            addr: address,
            rpc_password: Some("test-only".into()),
            command: String::new(),
        },
        commands,
    )
}

#[tokio::test]
async fn teardown_rejects_failed_command() {
    let (config, _) = fake_control(true, false);
    let result = rpc::teardown_circuit(&config, "42").await.unwrap();
    assert!(
        !result,
        "Authentication success must not mask command rejection"
    );
}

#[tokio::test]
async fn extend_rejects_error_text_instead_of_returning_it_as_id() {
    let (config, _) = fake_control(true, false);
    let result = rpc::extend_paid_circuit(&config, "+EXTENDPAIDCIRCUIT 0\ninvalid\n.".into()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn monitor_exit_restores_automatic_attachment() {
    let (config, commands) = fake_control(false, true);
    let handle = rpc::start_stream_attachment_monitor(config, "41".into(), "42".into())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .unwrap()
        .unwrap();
    let commands = commands.lock().unwrap();
    assert!(commands
        .iter()
        .any(|c| c == "SETCONF __LeaveStreamsUnattached=1"));
    assert!(commands
        .iter()
        .any(|c| c.contains("__LeaveStreamsUnattached=0")));
}

#[tokio::test]
async fn configuration_ignores_auth_and_quit_frames_and_selects_default_wallet() {
    let (config, _) = fake_control(false, false);
    assert_eq!(
        rpc::get_conf_payment_circuit_max_fee(&config)
            .await
            .unwrap(),
        1000
    );
    let (kind, _) = eltor::lightning::lookup_default_lightning_node_from_torrc(&config)
        .await
        .unwrap();
    assert_eq!(kind, "phoenixd");
}

#[tokio::test]
async fn control_endpoint_accepts_bare_ipv4_and_ipv6_ports() {
    let path =
        std::env::temp_dir().join(format!("eltor-control-endpoint-{}", rand::random::<u64>()));
    for (value, expected) in [
        ("9996", "127.0.0.1:9996"),
        ("127.0.0.1:9996", "127.0.0.1:9996"),
        ("[::1]:9996", "[::1]:9996"),
    ] {
        std::fs::write(&path, format!("ControlPort {}\n", value)).unwrap();
        assert_eq!(
            rpc::get_rpc_config_from_torrc(path.to_str().unwrap(), None)
                .await
                .unwrap()
                .addr,
            expected
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn directory_events_and_closed_stream_races_do_not_stop_monitor() {
    let (config, commands)=fake_control_with_events(false,false,
        "650 STREAM 41 NEW 0 relay:80 PURPOSE=DIR_FETCH\r\n650 STREAM 42 NEW 0 site:80 PURPOSE=USER\r\n650 STREAM 43 NEW 0 site:80 PURPOSE=USER\r\n");
    let handle = rpc::start_stream_attachment_monitor(config, "11".into(), "12".into())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if commands
                .lock()
                .unwrap()
                .iter()
                .any(|c| c.starts_with("ATTACHSTREAM 43 "))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!handle.is_finished());
    assert!(!commands
        .lock()
        .unwrap()
        .iter()
        .any(|c| c.starts_with("ATTACHSTREAM 41 ")));
    handle.abort();
}
