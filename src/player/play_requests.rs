use std::collections::BTreeMap;

use serenity::model::id::{ChannelId, UserId};
use tokio::{
    sync::{Mutex, Notify, oneshot},
    task::AbortHandle,
};

use crate::{error::AppError, player::track::ResolvedTrack, sources::resolver::TrackResolution};

#[derive(Clone, Copy)]
pub struct PlayRequestReservation {
    pub sequence: u64,
    pub session_epoch: u64,
}

pub struct PlayRequestTicket {
    pub reservation: PlayRequestReservation,
    pub response: oneshot::Receiver<Result<PlayCommitReceipt, AppError>>,
}

pub struct PendingPlayRequest {
    pub reservation: PlayRequestReservation,
    pub channel_id: ChannelId,
    pub requested_by: UserId,
    pub resolution: Result<TrackResolution, AppError>,
    pub response: oneshot::Sender<Result<PlayCommitReceipt, AppError>>,
}

pub struct PlayCommitReceipt {
    pub first_track: ResolvedTrack,
    pub requested_by: UserId,
    pub first_position: usize,
    pub added: usize,
    pub unavailable: usize,
    pub omitted: usize,
}

pub enum PlayRequestCancellation {
    Canceled { should_drain: bool },
    Forbidden,
    NotFound,
}

pub(crate) enum PlayRequestAbandonment {
    Abandoned { should_drain: bool },
    NotFound,
}

pub struct PlayRequestSequencer {
    inner: Mutex<SequencerState>,
    changed: Notify,
}

struct SequencerState {
    next_reservation: u64,
    next_commit: u64,
    slots: BTreeMap<u64, PlayRequestSlot>,
    draining: bool,
}

enum PlayRequestSlot {
    Resolving(ReservedPlayRequest),
    Ready(PendingPlayRequest),
}

struct ReservedPlayRequest {
    reservation: PlayRequestReservation,
    channel_id: ChannelId,
    requested_by: UserId,
    abort_handle: Option<AbortHandle>,
    response: oneshot::Sender<Result<PlayCommitReceipt, AppError>>,
}

struct RemovedPlayRequest {
    canceled: CanceledPlayRequest,
    should_drain: bool,
}

struct CanceledPlayRequest {
    abort_handle: Option<AbortHandle>,
    response: oneshot::Sender<Result<PlayCommitReceipt, AppError>>,
}

impl PlayRequestSequencer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(SequencerState {
                next_reservation: 0,
                next_commit: 0,
                slots: BTreeMap::new(),
                draining: false,
            }),
            changed: Notify::new(),
        }
    }

    pub async fn reserve(
        &self,
        session_epoch: u64,
        channel_id: ChannelId,
        requested_by: UserId,
    ) -> PlayRequestTicket {
        let mut state = self.inner.lock().await;
        let sequence = state.next_reservation;
        state.next_reservation = state.next_reservation.wrapping_add(1);
        let reservation = PlayRequestReservation {
            sequence,
            session_epoch,
        };
        let (response, receiver) = oneshot::channel();
        state.slots.insert(
            sequence,
            PlayRequestSlot::Resolving(ReservedPlayRequest {
                reservation,
                channel_id,
                requested_by,
                abort_handle: None,
                response,
            }),
        );
        PlayRequestTicket {
            reservation,
            response: receiver,
        }
    }

    pub async fn install_abort_handle(
        &self,
        reservation: PlayRequestReservation,
        abort_handle: AbortHandle,
    ) -> bool {
        let mut state = self.inner.lock().await;
        let Some(PlayRequestSlot::Resolving(request)) = state.slots.get_mut(&reservation.sequence)
        else {
            abort_handle.abort();
            return false;
        };
        if request.reservation.session_epoch != reservation.session_epoch {
            abort_handle.abort();
            return false;
        }
        request.abort_handle = Some(abort_handle);
        true
    }

    pub async fn publish(
        &self,
        reservation: PlayRequestReservation,
        resolution: Result<TrackResolution, AppError>,
    ) -> bool {
        {
            let mut state = self.inner.lock().await;
            let Some(PlayRequestSlot::Resolving(reserved)) =
                state.slots.remove(&reservation.sequence)
            else {
                return false;
            };
            if reserved.reservation.session_epoch != reservation.session_epoch {
                let _ = reserved
                    .response
                    .send(Err(obsolete_request_error(reservation)));
                state.advance_over_gaps();
                self.changed.notify_waiters();
                return false;
            }

            state.slots.insert(
                reservation.sequence,
                PlayRequestSlot::Ready(PendingPlayRequest {
                    reservation,
                    channel_id: reserved.channel_id,
                    requested_by: reserved.requested_by,
                    resolution,
                    response: reserved.response,
                }),
            );
        }
        self.wait_for_drain_role(reservation.sequence).await
    }

    pub async fn cancel(
        &self,
        reservation: PlayRequestReservation,
        requested_by: UserId,
    ) -> PlayRequestCancellation {
        let mut state = self.inner.lock().await;
        let Some(slot) = state.matching_slot(reservation) else {
            return PlayRequestCancellation::NotFound;
        };
        if slot.requested_by() != requested_by {
            return PlayRequestCancellation::Forbidden;
        }
        let Some(removed) = state.remove_request(reservation) else {
            return PlayRequestCancellation::NotFound;
        };
        drop(state);

        let RemovedPlayRequest {
            canceled,
            should_drain,
        } = removed;
        self.finish_removal(canceled, canceled_request_error());
        PlayRequestCancellation::Canceled { should_drain }
    }

    pub(crate) async fn abandon(
        &self,
        reservation: PlayRequestReservation,
    ) -> PlayRequestAbandonment {
        let removed = {
            let mut state = self.inner.lock().await;
            state.remove_request(reservation)
        };
        let Some(removed) = removed else {
            return PlayRequestAbandonment::NotFound;
        };

        let RemovedPlayRequest {
            canceled,
            should_drain,
        } = removed;
        self.finish_removal(canceled, abandoned_request_error(reservation));
        PlayRequestAbandonment::Abandoned { should_drain }
    }

    pub async fn has_outstanding_work(&self) -> bool {
        let state = self.inner.lock().await;
        state.draining || !state.slots.is_empty()
    }

    pub async fn take_next(&self) -> Option<PendingPlayRequest> {
        let mut state = self.inner.lock().await;
        let next_commit = state.next_commit;
        let Some(PlayRequestSlot::Ready(request)) = state.slots.remove(&next_commit) else {
            state.draining = false;
            self.changed.notify_waiters();
            return None;
        };
        state.next_commit = state.next_commit.wrapping_add(1);
        state.advance_over_gaps();
        self.changed.notify_waiters();
        Some(request)
    }

    pub async fn invalidate_before_epoch(&self, current_epoch: u64) -> usize {
        let canceled = {
            let mut state = self.inner.lock().await;
            let obsolete_sequences: Vec<u64> = state
                .slots
                .iter()
                .filter_map(|(sequence, slot)| {
                    (slot.session_epoch() != current_epoch).then_some(*sequence)
                })
                .collect();
            let canceled: Vec<_> = obsolete_sequences
                .into_iter()
                .filter_map(|sequence| state.slots.remove(&sequence))
                .map(PlayRequestSlot::into_canceled)
                .collect();
            state.advance_over_gaps();
            canceled
        };
        self.changed.notify_waiters();
        let canceled_count = canceled.len();
        for request in canceled {
            request.finish(session_changed_error(current_epoch));
        }
        canceled_count
    }

    fn finish_removal(&self, canceled: CanceledPlayRequest, error: AppError) {
        self.changed.notify_waiters();
        canceled.finish(error);
    }

    async fn wait_for_drain_role(&self, sequence: u64) -> bool {
        loop {
            let notification = self.changed.notified();
            tokio::pin!(notification);
            notification.as_mut().enable();
            {
                let mut state = self.inner.lock().await;
                if !state.slots.contains_key(&sequence) || state.draining {
                    return false;
                }
                if state.start_draining_if_ready() {
                    return true;
                }
            }
            notification.await;
        }
    }
}

impl Default for PlayRequestSequencer {
    fn default() -> Self {
        Self::new()
    }
}

impl SequencerState {
    fn matching_slot(&self, reservation: PlayRequestReservation) -> Option<&PlayRequestSlot> {
        self.slots
            .get(&reservation.sequence)
            .filter(|slot| slot.session_epoch() == reservation.session_epoch)
    }

    fn remove_request(
        &mut self,
        reservation: PlayRequestReservation,
    ) -> Option<RemovedPlayRequest> {
        self.matching_slot(reservation)?;
        let slot = self.slots.remove(&reservation.sequence)?;
        self.advance_over_gaps();
        Some(RemovedPlayRequest {
            canceled: slot.into_canceled(),
            should_drain: self.start_draining_if_ready(),
        })
    }

    fn advance_over_gaps(&mut self) {
        while self.next_commit < self.next_reservation
            && !self.slots.contains_key(&self.next_commit)
        {
            self.next_commit = self.next_commit.wrapping_add(1);
        }
    }

    fn start_draining_if_ready(&mut self) -> bool {
        if self.draining
            || !matches!(
                self.slots.get(&self.next_commit),
                Some(PlayRequestSlot::Ready(_))
            )
        {
            return false;
        }
        self.draining = true;
        true
    }
}

impl PlayRequestSlot {
    fn session_epoch(&self) -> u64 {
        match self {
            Self::Resolving(request) => request.reservation.session_epoch,
            Self::Ready(request) => request.reservation.session_epoch,
        }
    }

    fn requested_by(&self) -> UserId {
        match self {
            Self::Resolving(request) => request.requested_by,
            Self::Ready(request) => request.requested_by,
        }
    }

    fn into_canceled(self) -> CanceledPlayRequest {
        match self {
            Self::Resolving(request) => CanceledPlayRequest {
                abort_handle: request.abort_handle,
                response: request.response,
            },
            Self::Ready(request) => CanceledPlayRequest {
                abort_handle: None,
                response: request.response,
            },
        }
    }
}

impl CanceledPlayRequest {
    fn finish(self, error: AppError) {
        if let Some(abort_handle) = self.abort_handle {
            abort_handle.abort();
        }
        let _ = self.response.send(Err(error));
    }
}

fn canceled_request_error() -> AppError {
    AppError::Canceled {
        operation: "play request",
    }
}

fn abandoned_request_error(reservation: PlayRequestReservation) -> AppError {
    AppError::Internal {
        context: format!(
            "play request sequence {} for session {} was abandoned before commit",
            reservation.sequence, reservation.session_epoch
        ),
    }
}

fn obsolete_request_error(reservation: PlayRequestReservation) -> AppError {
    AppError::Internal {
        context: format!(
            "play request sequence {} for session {} became obsolete",
            reservation.sequence, reservation.session_epoch
        ),
    }
}

fn session_changed_error(current_epoch: u64) -> AppError {
    AppError::Voice {
        context: format!("guild playback session changed to epoch {current_epoch}"),
    }
}
