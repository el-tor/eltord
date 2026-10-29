//! Private, leased observations for an in-process UI. These never authorize
//! traffic, payments, retries or fallback and contain no financial identifiers.
use serde::Serialize;
use std::sync::{Arc, Mutex, Weak};
use tokio::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FoundationClientStatus {
    NotConfigured,
    Starting,
    Recovering,
    LastVerifiedActive,
    Closed,
    Stopped,
}

struct Status {
    owner: Option<Weak<()>>,
    phase: FoundationClientStatus,
    expires: Option<Instant>,
}
static STATUS: Mutex<Status> = Mutex::new(Status {
    owner: None,
    phase: FoundationClientStatus::NotConfigured,
    expires: None,
});

fn status_at(now: Instant) -> FoundationClientStatus {
    let Ok(status) = STATUS.lock() else {
        return FoundationClientStatus::Recovering;
    };
    if status.phase == FoundationClientStatus::LastVerifiedActive
        && status.expires.map_or(true, |expires| now >= expires)
    {
        FoundationClientStatus::Recovering
    } else {
        status.phase
    }
}

/// Last authenticated in-process observation, conservatively expired against
/// monotonic time. This performs no Tor, wallet, filesystem or ledger operation.
pub fn foundation_client_status() -> FoundationClientStatus {
    status_at(Instant::now())
}

pub(super) struct StatusOwner(Arc<()>);
#[derive(Clone)]
pub(super) struct StatusPublisher(Weak<()>);
impl StatusOwner {
    pub(super) fn start() -> Self {
        let owner = Self(Arc::new(()));
        if let Ok(mut status) = STATUS.lock() {
            *status = Status {
                owner: Some(Arc::downgrade(&owner.0)),
                phase: FoundationClientStatus::Starting,
                expires: None,
            };
        }
        owner
    }
    pub(super) fn publisher(&self) -> StatusPublisher {
        StatusPublisher(Arc::downgrade(&self.0))
    }
}
impl StatusPublisher {
    pub(super) fn publish(&self, phase: FoundationClientStatus, expires: Option<Instant>) {
        // A publisher never owns its runtime generation. Cancellation/drop or
        // a newer owner makes every delayed update from the old task inert.
        let Some(_owner) = self.0.upgrade() else {
            return;
        };
        if let Ok(mut status) = STATUS.lock() {
            if status
                .owner
                .as_ref()
                .is_some_and(|owner| Weak::ptr_eq(owner, &self.0))
                && status.phase != FoundationClientStatus::Closed
            {
                status.phase = phase;
                status.expires = if phase == FoundationClientStatus::LastVerifiedActive {
                    expires
                } else {
                    None
                };
            }
        }
    }
}
impl Drop for StatusOwner {
    fn drop(&mut self) {
        if let Ok(mut status) = STATUS.lock() {
            if status
                .owner
                .as_ref()
                .is_some_and(|owner| Weak::ptr_eq(owner, &Arc::downgrade(&self.0)))
            {
                if status.phase != FoundationClientStatus::Closed {
                    status.phase = FoundationClientStatus::Stopped;
                }
                status.expires = None;
                status.owner = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn leases_expire_old_generations_cannot_publish_and_parent_abort_cancels_controller() {
        let old = StatusOwner::start();
        let publisher = old.publisher();
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        publisher.publish(FoundationClientStatus::LastVerifiedActive, Some(deadline));
        assert_eq!(
            status_at(deadline - std::time::Duration::from_millis(1)),
            FoundationClientStatus::LastVerifiedActive
        );
        assert_eq!(status_at(deadline), FoundationClientStatus::Recovering);
        // Republishing an unchanged observation does not grant fresh time.
        publisher.publish(FoundationClientStatus::LastVerifiedActive, Some(deadline));
        assert_eq!(status_at(deadline), FoundationClientStatus::Recovering);
        let current = StatusOwner::start();
        publisher.publish(FoundationClientStatus::Closed, None);
        drop(old);
        assert_eq!(foundation_client_status(), FoundationClientStatus::Starting);

        let (controller_tx, controller_rx) = tokio::sync::oneshot::channel();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let parent = tokio::spawn(async move {
            let mut controllers = crate::controller_tasks::OwnedControllers::default();
            let child = tokio::spawn(async move {
                let _owner = current;
                _owner.publisher().publish(
                    FoundationClientStatus::LastVerifiedActive,
                    Some(Instant::now() + std::time::Duration::from_secs(60)),
                );
                ready_tx.send(()).unwrap();
                std::future::pending::<()>().await;
            });
            controllers.track(&child);
            controller_tx.send(child).unwrap();
            std::future::pending::<()>().await;
        });
        let child = controller_rx.await.unwrap();
        ready_rx.await.unwrap();
        assert_eq!(
            foundation_client_status(),
            FoundationClientStatus::LastVerifiedActive
        );
        parent.abort();
        assert!(parent.await.unwrap_err().is_cancelled());
        assert!(child.await.unwrap_err().is_cancelled());
        assert_eq!(foundation_client_status(), FoundationClientStatus::Stopped);
        publisher.publish(FoundationClientStatus::LastVerifiedActive, Some(deadline));
        assert_eq!(foundation_client_status(), FoundationClientStatus::Stopped);
    }
}
