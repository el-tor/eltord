//! Runtime ownership of one explicitly approved foundation path. Reconnects
//! recover that path; failures never create fresh scopes or enable legacy pay.
use super::{
    activation::begin_path,
    construction::PreparedConstruction,
    foundation::{reconcile_origin_financial, ClientFoundation, FoundationPlan},
    handshake::reconcile_handshake_financial,
    runtime_config::ClientPlanConfig,
};
use crate::{
    relay::{observe_clock, FoundationConfig},
    rpc::PaymentControl,
    types::{Relay, RpcConfig},
};
use std::{error::Error, net::SocketAddr, path::Path, sync::Arc, time::Duration};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
pub(crate) async fn run(rpc: &RpcConfig) -> Result<()> {
    let path =
        std::env::var_os("ELTOR_FOUNDATION_CLIENT_PLAN").ok_or("Missing private client plan")?;
    let account_path = std::env::var_os("ELTOR_FOUNDATION_CONFIG")
        .ok_or("Missing foundation account configuration")?;
    run_with_paths(rpc, Path::new(&path), Path::new(&account_path)).await
}
/// Explicit attachment to existing standalone Tor. No environment mutation,
/// embedded Tor startup, ledger provisioning or replacement ownership scope.
pub async fn run_foundation_client_controller(
    control_address: SocketAddr,
    password_file: &Path,
    client_plan: &Path,
    account_config: &Path,
) -> Result<()> {
    if client_plan.as_os_str().is_empty() || account_config.as_os_str().is_empty() {
        return Err("Explicit private client plan and account configuration required".into());
    }
    let rpc = crate::relay::local_controller_rpc(control_address, password_file)?;
    // Establish explicit authenticated mode before opening wallet resources.
    // The runtime independently authenticates again before live construction;
    // a process restart cannot substitute a numeric circuit for owned state.
    let mut control = PaymentControl::connect(&rpc).await?;
    if control.command("GETCONF ElTorFoundation").await? != "250 ElTorFoundation=1" {
        return Err("Standalone client requires authenticated foundation mode".into());
    }
    drop(control);
    run_with_paths(&rpc, client_plan, account_config).await
}
pub(crate) async fn run_with_paths(
    rpc: &RpcConfig,
    client_plan: &Path,
    account_config: &Path,
) -> Result<()> {
    let status = super::foundation_status::StatusOwner::start();
    let request = ClientPlanConfig::load(client_plan)?;
    let config = FoundationConfig::load(account_config)?;
    let (mut ledger, wallet, uncertainty) = config.open_outgoing().await?;
    ledger.configure_account_budget(request.account_budget())?;
    // Financial uncertainty survives Tor process loss. Reconcile existing native
    // records before attempting any live-path restoration, without fetch/pay.
    // Bound work per page and per startup; never silently skip a truncated scan.
    let mut cursor = None;
    let mut handshake_cursor = None;
    let mut recovery_finished = false;
    let mut rounds_finished = false;
    let mut handshakes_finished = false;
    for _ in 0..257 {
        let mut page_failed = false;
        if !rounds_finished {
            match reconcile_origin_financial(&mut ledger, &wallet, cursor, 16, || {
                observe_clock(uncertainty)
            })
            .await
            {
                Ok(page) => {
                    cursor = page.after;
                    rounds_finished = page.examined < 16;
                }
                Err(_) => {
                    page_failed = true;
                }
            }
        }
        if !handshakes_finished {
            match reconcile_handshake_financial(&mut ledger, &wallet, handshake_cursor, 16, || {
                observe_clock(uncertainty)
            })
            .await
            {
                Ok(page) => {
                    handshake_cursor = page.after;
                    handshakes_finished = page.examined < 16;
                }
                Err(_) => {
                    page_failed = true;
                }
            }
        }
        if page_failed {
            return Err("Financial recovery page failed; new work stopped".into());
        }
        if rounds_finished && handshakes_finished {
            recovery_finished = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if !recovery_finished {
        return Err("Financial recovery scan exceeded startup bound".into());
    }
    let mut control = PaymentControl::connect(rpc).await?;
    if control.command("GETCONF ElTorFoundation").await? != "250 ElTorFoundation=1" {
        return Err("Foundation mode changed".into());
    }
    let mut coordinator = if let Some((instance, handle, _)) =
        ledger.get_origin_plan_by_scope(request.scope)?
    {
        if ledger.binding_is_closed(instance, handle)? {
            status.publisher().publish(
                super::foundation_status::FoundationClientStatus::Closed,
                None,
            );
        }
        let saved = PreparedConstruction::restore(&ledger, request.scope)?
            .ok_or("Missing original construction")?;
        if !request.matches_saved_context(&saved.accepted_context()?)? {
            return Err("Client plan differs from accepted path".into());
        }
        let plan = FoundationPlan::restore(&mut ledger, instance, handle)?;
        ClientFoundation::new(ledger, control, Some(Arc::new(wallet)), plan)?
    } else {
        if ledger.get_origin_construction(request.scope)?.is_some() {
            return Err("Construction outcome requires recovery; no replacement authorized".into());
        }
        let relays = request
            .fingerprints
            .iter()
            .map(|fp| Relay {
                nickname: String::new(),
                fingerprint: hex::encode(fp),
                contact: None,
                bandwidth: None,
                ip: None,
                port: None,
                payment_bolt12_offer: None,
                payment_bip353: None,
                payment_bolt11_lnurl: None,
                payment_bolt11_lightning_address: None,
                payment_rate_msats: None,
                payment_interval_seconds: None,
                payment_interval_rounds: None,
                payment_handshake_fee: None,
                payment_handshake_fee_payhash: None,
                payment_handshake_fee_preimage: None,
                payment_id_hashes_10: None,
                relay_tag: None,
                hop: None,
            })
            .collect();
        begin_path(
            rpc,
            ledger,
            control,
            Some(Arc::new(wallet)),
            relays,
            request.scope,
            request.limits,
            request.routing_fee_per_hop_msat,
            request.handshake_msat,
            request.handshake_funding_timeout_ms,
            || observe_clock(uncertainty),
        )
        .await?
    };
    coordinator.attach_status(status.publisher());
    let mut backoff = 1u64;
    let mut financial_cursor = None;
    let mut handshake_financial_cursor = None;
    let mut next_financial = tokio::time::Instant::now();
    loop {
        if tokio::time::Instant::now() >= next_financial {
            // This page runs even when Tor recovery fails; loss of capacity
            // never implies a refund or releases an uncertain payment.
            let mut recovery_failed = match coordinator
                .reconcile_financial(financial_cursor, 16, || observe_clock(uncertainty))
                .await
            {
                Ok(page) => {
                    financial_cursor = if page.examined < 16 { None } else { page.after };
                    false
                }
                Err(_) => true,
            };
            // Separate cursor: an outgoing-round operation must never skip an
            // earlier handshake operation (or vice versa). Attempt this page
            // even when round recovery failed, then stop new capacity work if
            // either exact financial reconciliation encountered an error.
            match coordinator
                .reconcile_handshake_financial(handshake_financial_cursor, 16, || {
                    observe_clock(uncertainty)
                })
                .await
            {
                Ok(page) => {
                    handshake_financial_cursor = if page.examined < 16 { None } else { page.after };
                }
                Err(_) => {
                    recovery_failed = true;
                }
            }
            next_financial = tokio::time::Instant::now() + Duration::from_secs(5);
            if recovery_failed {
                coordinator.report_status_failure();
                log::warn!("Foundation financial recovery incomplete; new work stopped");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        }
        let failed = match coordinator.step(|| observe_clock(uncertainty)).await {
            Ok(_) => {
                coordinator.report_status();
                backoff = 1;
                false
            }
            Err(_) => {
                coordinator.report_status_failure();
                log::warn!("Foundation client stopped new work; reconciling original path");
                true
            }
        };
        if failed {
            tokio::time::sleep(Duration::from_secs(backoff)).await;
            backoff = (backoff * 2).min(30);
            if let Ok(mut connection) = PaymentControl::connect(rpc).await {
                if connection
                    .command("GETCONF ElTorFoundation")
                    .await
                    .ok()
                    .as_deref()
                    == Some("250 ElTorFoundation=1")
                {
                    coordinator.reconnect(connection);
                }
            }
        } else {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}
