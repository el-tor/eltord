//! Private, leased observations for an in-process UI. These never authorize
//! traffic, payments, retries or fallback and contain no financial identifiers.
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use tokio::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
struct Registry(Mutex<Status>, bool);
static STATUS: LazyLock<Arc<Registry>> = LazyLock::new(|| Arc::new(Registry::new(true)));
impl Registry {
    fn new(export: bool) -> Self {
        Self(
            Mutex::new(Status {
                owner: None,
                phase: FoundationClientStatus::NotConfigured,
                expires: None,
            }),
            export,
        )
    }
    fn at(&self, now: Instant) -> FoundationClientStatus {
        let Ok(status) = self.0.lock() else {
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
}

/// Last authenticated in-process observation, conservatively expired against
/// monotonic time. This performs no Tor, wallet, filesystem or ledger operation.
pub fn foundation_client_status() -> FoundationClientStatus {
    STATUS.at(Instant::now())
}

pub(super) struct StatusOwner {
    token: Arc<()>,
    registry: Arc<Registry>,
}
#[derive(Clone)]
pub(super) struct StatusPublisher {
    token: Weak<()>,
    registry: Arc<Registry>,
}
impl StatusOwner {
    pub(super) fn start() -> Self {
        Self::start_in(STATUS.clone())
    }
    fn start_in(registry: Arc<Registry>) -> Self {
        let owner = Self {
            token: Arc::new(()),
            registry,
        };
        if let Ok(mut status) = owner.registry.0.lock() {
            *status = Status {
                owner: Some(Arc::downgrade(&owner.token)),
                phase: FoundationClientStatus::Starting,
                expires: None,
            };
            if owner.registry.1 {
                crate::foundation_status_bridge::publish(FoundationClientStatus::Starting, None);
            }
        }
        owner
    }
    pub(super) fn publisher(&self) -> StatusPublisher {
        StatusPublisher {
            token: Arc::downgrade(&self.token),
            registry: self.registry.clone(),
        }
    }
}
impl StatusPublisher {
    pub(super) fn publish(&self, phase: FoundationClientStatus, expires: Option<Instant>) {
        // A publisher never owns its runtime generation. Cancellation/drop or
        // a newer owner makes every delayed update from the old task inert.
        let Some(_owner) = self.token.upgrade() else {
            return;
        };
        if let Ok(mut status) = self.registry.0.lock() {
            if status
                .owner
                .as_ref()
                .is_some_and(|owner| Weak::ptr_eq(owner, &self.token))
                && status.phase != FoundationClientStatus::Closed
            {
                let changed = status.phase != phase || status.expires != expires;
                status.phase = phase;
                status.expires = if phase == FoundationClientStatus::LastVerifiedActive {
                    expires
                } else {
                    None
                };
                if changed && self.registry.1 {
                    crate::foundation_status_bridge::publish(status.phase, status.expires);
                }
            }
        }
    }
}
impl Drop for StatusOwner {
    fn drop(&mut self) {
        if let Ok(mut status) = self.registry.0.lock() {
            if status
                .owner
                .as_ref()
                .is_some_and(|owner| Weak::ptr_eq(owner, &Arc::downgrade(&self.token)))
            {
                if status.phase != FoundationClientStatus::Closed {
                    status.phase = FoundationClientStatus::Stopped;
                }
                status.expires = None;
                status.owner = None;
                if self.registry.1 {
                    crate::foundation_status_bridge::publish(status.phase, None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn leases_expire_old_generations_cannot_publish_and_parent_abort_cancels_controller() {
        let registry = Arc::new(Registry::new(false));
        let old = StatusOwner::start_in(registry.clone());
        let publisher = old.publisher();
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        publisher.publish(FoundationClientStatus::LastVerifiedActive, Some(deadline));
        assert_eq!(
            registry.at(deadline - std::time::Duration::from_millis(1)),
            FoundationClientStatus::LastVerifiedActive
        );
        assert_eq!(registry.at(deadline), FoundationClientStatus::Recovering);
        // Republishing an unchanged observation does not grant fresh time.
        publisher.publish(FoundationClientStatus::LastVerifiedActive, Some(deadline));
        assert_eq!(registry.at(deadline), FoundationClientStatus::Recovering);
        let current = StatusOwner::start_in(registry.clone());
        publisher.publish(FoundationClientStatus::Closed, None);
        drop(old);
        assert_eq!(
            registry.at(Instant::now()),
            FoundationClientStatus::Starting
        );

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
            registry.at(Instant::now()),
            FoundationClientStatus::LastVerifiedActive
        );
        parent.abort();
        assert!(parent.await.unwrap_err().is_cancelled());
        assert!(child.await.unwrap_err().is_cancelled());
        assert_eq!(registry.at(Instant::now()), FoundationClientStatus::Stopped);
        publisher.publish(FoundationClientStatus::LastVerifiedActive, Some(deadline));
        assert_eq!(registry.at(Instant::now()), FoundationClientStatus::Stopped);
    }
}
