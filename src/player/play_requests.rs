use std::collections::BTreeMap;

use serenity::model::id::{ChannelId, UserId};
use tokio::sync::{Mutex, Notify, oneshot};

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
                response,
            }),
        );
        PlayRequestTicket {
            reservation,
            response: receiver,
        }
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

    pub async fn take_next(&self) -> Option<PendingPlayRequest> {
        let mut state = self.inner.lock().await;
        let next_commit = state.next_commit;
        let Some(PlayRequestSlot::Ready(request)) = state.slots.remove(&next_commit) else {
            state.draining = false;
            return None;
        };
        state.next_commit = state.next_commit.wrapping_add(1);
        state.advance_over_gaps();
        self.changed.notify_waiters();
        Some(request)
    }

    pub async fn invalidate_before_epoch(&self, current_epoch: u64) -> usize {
        let responses = {
            let mut state = self.inner.lock().await;
            let obsolete_sequences: Vec<u64> = state
                .slots
                .iter()
                .filter_map(|(sequence, slot)| {
                    (slot.session_epoch() != current_epoch).then_some(*sequence)
                })
                .collect();
            let responses: Vec<_> = obsolete_sequences
                .into_iter()
                .filter_map(|sequence| state.slots.remove(&sequence))
                .map(PlayRequestSlot::response)
                .collect();
            state.advance_over_gaps();
            responses
        };
        self.changed.notify_waiters();
        let canceled = responses.len();
        for response in responses {
            let _ = response.send(Err(session_changed_error(current_epoch)));
        }
        canceled
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

    fn response(self) -> oneshot::Sender<Result<PlayCommitReceipt, AppError>> {
        match self {
            Self::Resolving(request) => request.response,
            Self::Ready(request) => request.response,
        }
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
