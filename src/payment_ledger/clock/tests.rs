use super::*;
use std::path::PathBuf;
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("eltor-clock-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("account.sqlite")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn account() -> AccountId {
    AccountId([9; 32])
}
fn batch() -> BatchRegistration {
    BatchRegistration {
        period: 0,
        request_digest: [3; 32],
        rounds: std::array::from_fn(|n| Obligation {
            instance: [1; 16],
            handle: [2; 16],
            circuit_binding: [3; 32],
            round: n as u64 + 1,
            kind: ChargeKind::Round,
            payment_id: [n as u8 + 4; 32],
            amount_msat: 0,
            expected_payment_hash: None,
            policy_hash: [14; 32],
            allocation_hash: [15; 32],
            window: None,
        }),
    }
}
fn initial() -> InitialClockAnchor {
    InitialClockAnchor {
        constructed_at: UtcClockInterval {
            earliest_ms: 100_000,
            latest_ms: 100_010,
        },
        committed_at: UtcClockInterval {
            earliest_ms: 110_000,
            latest_ms: 110_010,
        },
        setup_timeout_ms: 60_001,
    }
}
fn activation() -> ActivationIntervalAnchor {
    ActivationIntervalAnchor {
        started_at: UtcClockInterval {
            earliest_ms: 130_000,
            latest_ms: 130_017,
        },
        round_ms: 60_000,
        early_ms: 60_000,
        grace_ms: 15_013,
    }
}
fn setup(dir: &Directory) -> PaymentLedger {
    let mut ledger = PaymentLedger::provision(&dir.file(), account()).unwrap();
    ledger.reserve_batch(&batch(), [16; 16]).unwrap();
    ledger
}
fn authorize_first(ledger: &mut PaymentLedger) {
    ledger
        .authorize_zero(
            &batch().rounds[0],
            [17; 16],
            ClockObservation {
                now_ms: 120_000,
                uncertainty_ms: 0,
            },
        )
        .unwrap();
}
#[test]
fn immutable_interval_anchors_preserve_original_millisecond_windows_on_restart() {
    let dir = Directory::new();
    let mut ledger = setup(&dir);
    ledger
        .bind_initial_clock_anchor([1; 16], [2; 16], initial())
        .unwrap();
    let window = ledger
        .get_initial_window([1; 16], [2; 16])
        .unwrap()
        .unwrap();
    assert!(window.opens_at_ms == 110_010 && window.closes_at_ms == 160_001);
    authorize_first(&mut ledger);
    ledger
        .bind_activation_interval_anchor([1; 16], [2; 16], activation())
        .unwrap();
    let round2 = activation().acceptance_window(2).unwrap();
    assert!(round2.opens_at_ms == 130_017 && round2.closes_at_ms == 205_013);
    ledger
        .authorize_zero(
            &batch().rounds[1],
            [18; 16],
            ClockObservation {
                now_ms: 190_000,
                uncertainty_ms: 0,
            },
        )
        .unwrap();
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    assert!(ledger.get_initial_clock_anchor([1; 16], [2; 16]).unwrap() == Some(initial()));
    assert!(
        ledger
            .get_activation_interval_anchor([1; 16], [2; 16])
            .unwrap()
            == Some(activation())
    );
    assert!(ledger
        .bind_initial_clock_anchor([1; 16], [2; 16], initial())
        .is_ok());
    assert!(ledger
        .bind_activation_interval_anchor([1; 16], [2; 16], activation())
        .is_ok());
    let mut changed = initial();
    changed.committed_at.earliest_ms += 1;
    assert!(matches!(
        ledger.bind_initial_clock_anchor([1; 16], [2; 16], changed),
        Err(LedgerError::Conflict)
    ));
    let mut changed = activation();
    changed.started_at.latest_ms -= 1;
    assert!(matches!(
        ledger.bind_activation_interval_anchor([1; 16], [2; 16], changed),
        Err(LedgerError::Conflict)
    ));
}
#[test]
fn broad_invalid_or_unproven_activation_is_never_bound_and_policy_grace_is_exact() {
    let dir = Directory::new();
    let mut ledger = setup(&dir);
    ledger
        .bind_initial_clock_anchor([1; 16], [2; 16], initial())
        .unwrap();
    assert!(ledger
        .bind_activation_interval_anchor([1; 16], [2; 16], activation())
        .is_err());
    authorize_first(&mut ledger);
    let mut accepted = activation();
    accepted.grace_ms = 60_000;
    ledger
        .bind_activation_interval_anchor([1; 16], [2; 16], accepted)
        .unwrap();
    assert!(
        ledger
            .get_activation_interval_anchor([1; 16], [2; 16])
            .unwrap()
            .unwrap()
            .grace_ms
            == 60_000
    );
    assert!(accepted.acceptance_window(2).unwrap().closes_at_ms == 250_000);
    accepted.grace_ms = 60_001;
    assert!(accepted.acceptance_window(2).is_err());
    let mut broad = activation();
    broad.started_at.latest_ms = 300_000;
    assert!(broad.acceptance_window(2).is_err());
    let mut invalid = initial();
    invalid.constructed_at.earliest_ms = -1;
    assert!(invalid.acceptance_window().is_err());
}
#[test]
fn point_and_interval_apis_cannot_install_competing_anchors_or_reset_after_closure() {
    let dir = Directory::new();
    let mut ledger = setup(&dir);
    ledger
        .bind_initial_clock_anchor([1; 16], [2; 16], initial())
        .unwrap();
    authorize_first(&mut ledger);
    ledger
        .bind_activation_interval_anchor([1; 16], [2; 16], activation())
        .unwrap();
    assert!(matches!(
        ledger.bind_activation(
            [1; 16],
            [2; 16],
            ActivationAnchor {
                started_at_ms: 130_000,
                round_ms: 60_000,
                early_ms: 60_000,
                grace_ms: 15_013
            }
        ),
        Err(LedgerError::Conflict)
    ));
    let mut changed = initial();
    changed.setup_timeout_ms += 1;
    ledger.close_instance([1; 16]).unwrap();
    assert!(ledger
        .bind_initial_clock_anchor([1; 16], [2; 16], changed)
        .is_err());
    assert!(ledger
        .bind_initial_clock_anchor([1; 16], [2; 16], initial())
        .is_ok());
}
#[test]
fn explicit_v3_migration_preserves_exact_legacy_points_and_refuses_fabricated_intervals() {
    let dir = Directory::new();
    let mut ledger = setup(&dir);
    let window = AcceptanceWindow {
        opens_at_ms: 110_000,
        closes_at_ms: 160_000,
    };
    ledger
        .bind_initial_window([1; 16], [2; 16], window)
        .unwrap();
    authorize_first(&mut ledger);
    let point = ActivationAnchor {
        started_at_ms: 130_000,
        round_ms: 60_000,
        early_ms: 60_000,
        grace_ms: 15_000,
    };
    ledger.bind_activation([1; 16], [2; 16], point).unwrap();
    ledger.connection.execute_batch("DROP TABLE initial_clock_anchors;DROP TABLE activation_interval_anchors;PRAGMA user_version=3;").unwrap();
    drop(ledger);
    assert!(PaymentLedger::reopen(&dir.file(), account()).is_err());
    assert!(PaymentLedger::upgrade_from_v3(&dir.file(), AccountId([8; 32])).is_err());
    let mut ledger = PaymentLedger::upgrade_from_v3(&dir.file(), account()).unwrap();
    assert!(ledger.get_initial_window([1; 16], [2; 16]).unwrap() == Some(window));
    assert!(ledger
        .get_initial_clock_anchor([1; 16], [2; 16])
        .unwrap()
        .is_none());
    assert!(
        ledger
            .get_activation_interval_anchor([1; 16], [2; 16])
            .unwrap()
            == Some(point.into())
    );
    assert!(ledger
        .bind_initial_clock_anchor([1; 16], [2; 16], initial())
        .is_err());
    assert!(ledger
        .bind_activation_interval_anchor([1; 16], [2; 16], activation())
        .is_err());
    drop(ledger);
    assert!(PaymentLedger::upgrade_from_v3(&dir.file(), account()).is_ok());
}
#[test]
fn corrupted_interval_metadata_cannot_reopen_or_widen_original_initial_window() {
    let dir = Directory::new();
    let mut ledger = setup(&dir);
    ledger
        .bind_initial_clock_anchor([1; 16], [2; 16], initial())
        .unwrap();
    ledger
        .connection
        .execute("UPDATE initial_windows SET closes_at_ms=closes_at_ms+1", [])
        .unwrap();
    drop(ledger);
    assert!(PaymentLedger::reopen(&dir.file(), account()).is_err());
}
