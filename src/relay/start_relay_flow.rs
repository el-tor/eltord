use lni::phoenixd::ListTransactionsParams;

// 1. Set Torrc Config
// 2. Handshake
// 3. Emit Event EXTENDPAIDCIRCUIT
// 4. Start Relay Payment Watcher
// 5. Init Payments Ledger
// 6. Start Lightning payment watcher
// 7. Payment Ledger Cron (Auditor Loop)
pub async fn start_relay_flow() {
    // TODO test gettting all payments received from lightning node

    let wallet = crate::lightning::load_wallet().await;

    let params = ListTransactionsParams {
        from: 0,
        until: 0,
        limit: 100,
        offset: 0,
        unpaid: false,
        invoice_type: "all".to_string(),
    };
    match wallet.list_transactions(params).await {
        Ok(txns) => {
            println!("Relay recent transactions: {:?}", txns);
        }
        Err(e) => {
            panic!("Failed to list transactions: {:?}", e);
        }
    }

}