//! Outbound adapters (transport, LLM clients, …).

pub(crate) mod iroh_gossip_relay_backend;

pub(crate) use iroh_gossip_relay_backend::IrohGossipRelayBackend;
