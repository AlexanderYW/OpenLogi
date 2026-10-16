//! The select loop and slot bookkeeping the gesture and keyboard capture
//! managers share.
//!
//! The two managers deliberately keep separate state machines (see
//! [`super::capture_session`]): what keys a slot, what a session targets and
//! dispatches, and what retiring one cancels all differ. What they wait on,
//! in which order, and how they keep a map of slots — one shared receiver
//! lease, owed restores retried and expedited, the earliest recovery deadline
//! — do not. [`run`] is that loop; a [`CaptureManager`] is the state machine
//! it drives; the free functions below are the slot bookkeeping.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Weak};

use openlogi_hid::{
    CaptureSessionOutcome, CaptureSessionStop, ChannelRegistry, DeviceIoGate, DeviceRoute,
    PendingCaptureRestore,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

use super::capture_session::CaptureSlot;
use super::retry::{RETRY_DELAY, wait_for_deadline};
use super::shutdown::ManagerCompletion;
use crate::receiver_access::{ReceiverAccess, ReceiverRequestState, SessionReceiverLease};

/// Firmware ownership a finished session could not release, and when its
/// manager may next try.
pub(super) struct PendingRestore {
    pub(super) token: PendingCaptureRestore,
    pub(super) retry_at: Instant,
}

/// One tracked device's capture slot, as every manager keeps it.
pub(super) type ManagedSlot<Target, Dispatch> =
    CaptureSlot<Target, Dispatch, PendingRestore, CaptureSessionStop>;

/// The shared receiver lease every session of one manager holds, acquiring
/// it when none of them is alive.
pub(super) fn acquire_session_lease(
    receiver_access: &ReceiverAccess,
    lease: &mut Weak<SessionReceiverLease>,
) -> Option<Arc<SessionReceiverLease>> {
    if let Some(existing) = lease.upgrade() {
        return Some(existing);
    }
    let fresh = Arc::new(receiver_access.try_acquire_for_session()?);
    *lease = Arc::downgrade(&fresh);
    Some(fresh)
}

/// The earliest instant recovery work in `slots` can make progress, or
/// `None` while an exclusive request or suspended device I/O parks it.
pub(super) fn next_deadline<Key, Target, Dispatch>(
    requests: ReceiverRequestState,
    device_io_allowed: bool,
    slots: &HashMap<Key, ManagedSlot<Target, Dispatch>>,
) -> Option<Instant> {
    if requests.any() || !device_io_allowed {
        return None;
    }
    slots
        .values()
        .filter_map(ManagedSlot::recovery)
        .filter_map(|recovery| recovery.next_deadline(|pending| pending.retry_at))
        .min()
}

/// Whether any slot still owes the firmware a restore.
pub(super) fn has_pending_restores<Key, Target, Dispatch>(
    slots: &HashMap<Key, ManagedSlot<Target, Dispatch>>,
) -> bool {
    slots.values().any(|slot| {
        slot.recovery()
            .is_some_and(|recovery| recovery.pending_restore.is_some())
    })
}

/// Make every owed restore in `slots` due now.
pub(super) fn expedite_pending_restores<Key, Target, Dispatch>(
    slots: &mut HashMap<Key, ManagedSlot<Target, Dispatch>>,
) {
    let now = Instant::now();
    for slot in slots.values_mut() {
        if let Some(pending) = slot
            .recovery_mut()
            .and_then(|recovery| recovery.pending_restore.as_mut())
        {
            pending.retry_at = now;
        }
    }
}

/// Retry every restore in `slots` due by `now`. `elect` names the route a
/// slot's restore should now follow — the device may have moved since its
/// session ended — or `None` to keep it on the token's own route; a
/// same-route retry keeps the replacement-only policy a channel change set.
pub(super) async fn retry_pending_restores<Key, Target, Dispatch>(
    slots: &mut HashMap<Key, ManagedSlot<Target, Dispatch>>,
    registry: &ChannelRegistry,
    now: Instant,
    elect: impl Fn(&Key) -> Option<DeviceRoute>,
) where
    Key: Eq + Hash + Clone,
{
    let due: Vec<_> = slots
        .iter()
        .filter(|(_, slot)| {
            slot.recovery()
                .and_then(|recovery| recovery.pending_restore.as_ref())
                .is_some_and(|pending| pending.retry_at <= now)
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in due {
        let Some(CaptureSlot::Recovering(mut recovery)) = slots.remove(&key) else {
            continue;
        };
        let Some(pending) = recovery.pending_restore.take() else {
            slots.insert(key, CaptureSlot::Recovering(recovery));
            continue;
        };
        let outcome = match elect(&key) {
            Some(route) if route != *pending.token.route() => {
                pending.token.retry_via(route, registry).await
            }
            _ => pending.token.retry(registry).await,
        };
        if let CaptureSessionOutcome::RestorePending(token) = outcome {
            recovery.pending_restore = Some(PendingRestore {
                token,
                retry_at: Instant::now() + RETRY_DELAY,
            });
        }
        if !recovery.is_empty() {
            slots.insert(key, CaptureSlot::Recovering(recovery));
        }
    }
}

/// One capture manager's state machine, as [`run`] drives it.
pub(super) trait CaptureManager {
    /// What the orchestrator publishes for this manager to keep armed.
    type Published: Clone;
    /// An ordered report from one of the manager's session tasks.
    type Event;

    /// Bring the tracked sessions in line with `published`. Only called while
    /// host device I/O is allowed.
    async fn reconcile(&mut self, requests: ReceiverRequestState, published: &Self::Published);

    /// Apply one session report. Returns whether it calls for a reconcile.
    fn handle_session_event(
        &mut self,
        event: Self::Event,
        device_io_allowed: bool,
        receiver_requests: &watch::Receiver<ReceiverRequestState>,
        published: &watch::Receiver<Self::Published>,
    ) -> bool;

    /// The next instant recovery work can make progress, if any.
    fn deadline(&self, requests: ReceiverRequestState, device_io_allowed: bool) -> Option<Instant>;

    /// Whether any finished session still owes the firmware a restore.
    fn has_pending_restores(&self) -> bool;

    /// Make every owed restore due now.
    fn expedite_pending_restores(&mut self);

    /// Stop every tracked session, wait for each ordered completion, then
    /// retain and retry owed restores until firmware ownership is released.
    async fn drain_for_shutdown(
        &mut self,
        events: &mut mpsc::UnboundedReceiver<Self::Event>,
        receiver_requests: &watch::Receiver<ReceiverRequestState>,
        published: &watch::Receiver<Self::Published>,
    );
}

/// Everything [`run`] waits on.
pub(super) struct ManagerInputs<M: CaptureManager> {
    /// The orchestrator's publication for this manager.
    pub(super) published: watch::Receiver<M::Published>,
    /// Exclusive receiver requests, which suspend capture while pairing or a
    /// host transition owns the receiver.
    pub(super) receiver_requests: watch::Receiver<ReceiverRequestState>,
    /// Inventory channel publications: a change can unblock an owed restore.
    pub(super) registry_changes: watch::Receiver<()>,
    /// Host device-I/O gate.
    pub(super) device_io: DeviceIoGate,
    /// Reports from the manager's session tasks.
    pub(super) events: mpsc::UnboundedReceiver<M::Event>,
    /// The process lifecycle's stop request.
    pub(super) shutdown: oneshot::Receiver<()>,
}

/// A registry change matters only while a restore is owed; otherwise this
/// never resolves. Returns whether the registry is still open.
async fn wait_for_registry_change(
    changes: &mut watch::Receiver<()>,
    has_pending_restore: bool,
) -> bool {
    if !has_pending_restore {
        return std::future::pending().await;
    }
    changes.changed().await.is_ok()
}

/// Drive `manager` until the lifecycle asks it to stop or one of its
/// control-plane sources closes. Runs for the lifetime of the process.
pub(super) async fn run<M: CaptureManager>(
    mut manager: M,
    inputs: ManagerInputs<M>,
) -> ManagerCompletion {
    let ManagerInputs {
        mut published,
        mut receiver_requests,
        mut registry_changes,
        mut device_io,
        mut events,
        mut shutdown,
    } = inputs;
    let mut reconcile = true;

    loop {
        if reconcile {
            reconcile = false;
            if device_io.allows_io() {
                let requests = *receiver_requests.borrow_and_update();
                let wanted = published.borrow_and_update().clone();
                manager.reconcile(requests, &wanted).await;
            }
        }

        let requests = *receiver_requests.borrow();
        let deadline = manager.deadline(requests, device_io.allows_io());
        if deadline.is_some_and(|deadline| deadline <= Instant::now()) {
            reconcile = true;
            continue;
        }
        let has_pending_restores = manager.has_pending_restores();

        tokio::select! {
            biased;

            _ = &mut shutdown => {
                manager
                    .drain_for_shutdown(&mut events, &receiver_requests, &published)
                    .await;
                return ManagerCompletion::Graceful;
            }
            Some(event) = events.recv() => {
                reconcile |= manager.handle_session_event(
                    event,
                    device_io.allows_io(),
                    &receiver_requests,
                    &published,
                );
            }
            result = published.changed() => match result {
                Ok(()) => reconcile = true,
                Err(_) => return ManagerCompletion::Unexpected,
            },
            result = receiver_requests.changed() => match result {
                Ok(()) => reconcile = true,
                Err(_) => return ManagerCompletion::Unexpected,
            },
            allowed = device_io.changed() => match allowed {
                Some(true) => reconcile = true,
                Some(false) => {}
                None => return ManagerCompletion::Unexpected,
            },
            open = wait_for_registry_change(&mut registry_changes, has_pending_restores) => {
                if !open {
                    return ManagerCompletion::Unexpected;
                }
                if device_io.allows_io() {
                    manager.expedite_pending_restores();
                    reconcile = true;
                }
            }
            () = wait_for_deadline(deadline) => {
                reconcile = true;
            }
        }
    }
}
