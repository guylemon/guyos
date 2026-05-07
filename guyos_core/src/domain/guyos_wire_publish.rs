//! Room-scoped publish sequencing and deduplication (**ADR 0003** — D2, O1, R3).
//!
//! The hub **must** use exactly **one** [`RoomPublishLedger`] shard per active `room_id`, shared by
//! every stream attached to that room, so deduplication and ordering remain consistent.

#![allow(dead_code)] // Instantiated from tests and future hub adapters; `RoomPublishLedger` is injected into dispatch.

use std::collections::HashMap;

use uuid::Uuid;

/// Ledger decision for one `(room_id, client_message_id)` pair (**ADR 0003** task 6.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishDecision {
    /// First acceptance of this `client_message_id` in this room: assign next `seq` (from 0).
    FirstAcceptance { seq: u64 },
    /// Idempotent retry (R3): echo stored `seq`; do not advance ordering cursor; no observer fan-out.
    IdempotentReplay { seq: u64 },
}

/// Room-scoped publish ledger (**ADR 0003** “Implementation freedom”).
///
/// Hub adapters inject `&mut dyn RoomPublishLedger` so one shard per active `room_id` can be shared
/// across connections. See [`RoomPublishState`] for the in-memory implementation used in tests and
/// simple in-process hubs.
pub trait RoomPublishLedger {
    /// Apply D2/R3/O1 for a valid UUID and known room. `text` is **not** part of the ledger key.
    fn accept_or_replay(
        &mut self,
        room_id: &str,
        client_message_id: &Uuid,
    ) -> PublishDecision;
}

#[derive(Debug, Default)]
struct RoomShard {
    /// Next sequence number to assign on first acceptance (**O1**); starts at `0`.
    next_seq: u64,
    /// Accepted `client_message_id` → authoritative `seq` for R3 replay.
    by_client_id: HashMap<Uuid, u64>,
}

/// In-memory registry: one map entry per `room_id` (**ADR 0003** ledger option 1).
#[derive(Debug, Default)]
pub struct RoomPublishState {
    rooms: HashMap<String, RoomShard>,
}

impl RoomPublishLedger for RoomPublishState {
    fn accept_or_replay(
        &mut self,
        room_id: &str,
        client_message_id: &Uuid,
    ) -> PublishDecision {
        let shard = self
            .rooms
            .entry(room_id.to_string())
            .or_insert_with(RoomShard::default);

        if let Some(&seq) = shard.by_client_id.get(client_message_id) {
            return PublishDecision::IdempotentReplay { seq };
        }

        let seq = shard.next_seq;
        shard.next_seq = shard
            .next_seq
            .checked_add(1)
            .expect("room seq counter overflow");
        shard.by_client_id.insert(*client_message_id, seq);
        PublishDecision::FirstAcceptance { seq }
    }
}
