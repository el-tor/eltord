//! Native wallet I/O and the one serialized controller advance independently.
//! Wallet work retains its original durable dispatch/lookup identity on a
//! verified peer connection. No SQLite transaction spans a native await.
use super::*;
use std::future::Future;

impl ClientFoundation {
    pub(super) fn require_wallet_status(&self) -> Result<()> {
        if self.wallet_status_failed {
            return Err("Authenticated status refresh failed during wallet work".into());
        }
        Ok(())
    }

    pub(super) async fn await_wallet<T, P, F>(&mut self, work: P, observe: &mut F) -> T
    where
        P: Future<Output = T>,
        T: Send,
        F: FnMut() -> Result<ClockObservation>,
    {
        // Before START there is no active DATA to preserve. On restart the
        // persisted QUERY indices identify an already-started path even before
        // this process has recovered its first fresh ACTIVE observation.
        let mut refresh =
            !self.wallet_status_failed && self.query_indices.iter().any(|index| *index > 0);
        let mut next = Instant::now() + self.status_poll_delay(Instant::now());
        tokio::pin!(work);
        loop {
            tokio::select! {
                biased;
                output = &mut work => return output,
                _ = tokio::time::sleep_until(next), if refresh => {
                    // No other task borrows this PaymentControl: complete each
                    // framed GET/REFRESH before polling the native future again.
                    let failed = self.refresh_during_wallet(observe).await.is_err();
                    if failed {
                        // Finish the existing financial attempt (or its native
                        // timeout), retaining uncertainty. Stop subsequent pay;
                        // ordinary inventory recovery must validate capacity.
                        self.wallet_status_failed = true;
                        self.recovery = None;
                        refresh = false;
                    } else {
                        let now = Instant::now();
                        next = now + self.status_poll_delay(now);
                    }
                }
            }
        }
    }

    async fn refresh_during_wallet<F>(&mut self, observe: &mut F) -> Result<()>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        for event in self.control.take_events()? {
            if let Some(recovery) = &mut self.recovery {
                recovery.observe(&event)?;
            }
        }
        // Only authenticated replies update the native projection. GRACE,
        // pending and a newer round remain real observations, never inferred
        // ACTIVE, financial consumption or an extension of a saved deadline.
        self.observe_active(observe).await?;
        Ok(())
    }
}
