use std::{
    collections::{VecDeque, vec_deque},
    time::SystemTime,
};

use crate::{error::AppError, player::track::QueuedTrack, sources::resolver::PreparedStream};

pub struct TrackQueue {
    tracks: VecDeque<QueuedTrack>,
    max_size: usize,
}

impl TrackQueue {
    pub fn new(max_size: usize) -> Result<Self, AppError> {
        if max_size == 0 {
            return Err(AppError::InvalidInput {
                reason: "queue maximum size must be positive, received 0".to_owned(),
            });
        }
        Ok(Self {
            tracks: VecDeque::with_capacity(max_size),
            max_size,
        })
    }

    pub fn add(&mut self, track: QueuedTrack) -> Result<usize, AppError> {
        if self.remaining_capacity() == 0 {
            return Err(AppError::QueueFull {
                limit: self.max_size,
            });
        }
        let position = self.len() + 1;
        self.tracks.push_back(track);
        Ok(position)
    }

    pub fn add_prefix(&mut self, tracks: Vec<QueuedTrack>) -> QueueInsertionReceipt {
        let added = tracks.len().min(self.remaining_capacity());
        let omitted = tracks.len() - added;
        let first_position = (added > 0).then(|| self.len() + 1);
        self.tracks.extend(tracks.into_iter().take(added));
        QueueInsertionReceipt {
            first_position,
            added,
            omitted,
        }
    }

    pub fn pop_next(&mut self) -> Option<QueuedTrack> {
        self.tracks.pop_front()
    }

    pub fn peek_next(&self) -> Option<&QueuedTrack> {
        self.tracks.front()
    }

    /// Attaches a prefetched stream to the first queued copy of the track that
    /// still lacks a usable one. Returns `false` when no such entry remains, for
    /// example because the track was skipped or the queue was cleared meanwhile.
    pub fn attach_stream(&mut self, track_id: &str, stream: PreparedStream) -> bool {
        let now = SystemTime::now();
        let Some(queued) = self.tracks.iter_mut().find(|queued| {
            queued.track.id == track_id
                && !queued
                    .track
                    .prepared_stream
                    .as_ref()
                    .is_some_and(|existing| existing.is_reusable_at(now))
        }) else {
            return false;
        };
        queued.track.prepared_stream = Some(Box::new(stream));
        true
    }

    pub(crate) fn restore_current_to_front(&mut self, track: QueuedTrack) {
        // Replaying history must not discard a waiting track when the user queue is full.
        self.tracks.push_front(track);
    }

    pub fn iter(&self) -> vec_deque::Iter<'_, QueuedTrack> {
        self.tracks.iter()
    }

    pub fn clear(&mut self) {
        self.tracks.clear();
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn remaining_capacity(&self) -> usize {
        self.max_size.saturating_sub(self.len())
    }

    pub(crate) fn max_size(&self) -> usize {
        self.max_size
    }
}

pub struct QueueInsertionReceipt {
    pub first_position: Option<usize>,
    pub added: usize,
    pub omitted: usize,
}
