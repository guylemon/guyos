Hexagonal Architecture Layout Recommendation

*guyos-core: Production-Ready Rust P2P Chat Library*

Transforming the Spike into a Maintainable, Testable, Multi-Frontend
Library

Why Hexagonal Architecture for This Project?

The current spike successfully validates the core idea (iroh-gossip +
uniffi + optional LLM relay), but mixes concerns: domain models live
alongside iroh setup, uniffi scaffolding, SSE parsing, and CLI
orchestration. This works for a prototype but will hinder evolution.

Key Benefits for guyos-core

- **Testability:** Mock the NetworkPort to unit-test ChatService without
starting iroh or hitting the network.
- **Multiple Frontends:** Same core powers the Swift iOS app (via
uniffi), the CLI example, and future Kotlin/Android or web (WASM)
adapters.
- **Swappable Infrastructure:** Replace iroh-gossip with libp2p,
QUIC-only, or even a mock for offline testing without touching
business logic.
- **Clear LLM Boundary:** LLM relay becomes an optional application
service that any frontend can enable/disable.
- **Long-term Maintainability:** New developers instantly understand
where to add features (domain rules vs. iroh details vs. FFI glue).

Hexagonal Principles Applied Here

1. **Domain at the Center:** Pure Rust structs/enums with business
  invariants (Message, Ticket, Name, ChatError). Zero external crates.
2. **Ports = Contracts:** Traits that define what the core needs from
  the outside world (NetworkPort, LlmPort) and what the outside world
    can call (ChatUseCases).
3. **Adapters = Glue:** Concrete implementations live in adapters/ and
  are the only place that know about iroh, reqwest, uniffi, clap, etc.
4. **Dependency Inversion:** Application services depend on port
  traits, never on concrete adapters. Wiring happens at the
    composition root.
5. **Inbound vs Outbound:** Driver adapters (uniffi, CLI) call into the
  core; driven adapters (iroh, OpenAI) are called by the core.

Recommended Directory & Module Layout

This layout balances Rust conventions, hexagonal separation, and
practicality for a library that is both a binary example and a uniffi
target.


|                             |                                                                                                                                                                                                     |
| --------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Directory / File**        | **Responsibility & Contents**                                                                                                                                                                       |
| **src/lib.rs**              | Public facade + composition root. Re-exports domain & application types. Provides factory functions (e.g. build_chat_service) that wire adapters. Contains uniffi::setup_scaffolding!()             |
| **src/domain/**             | Pure business logic. No external crates except std. Entities, value objects, domain events, and rich error types.                                                                                   |
| domain/chat.rs              | Message, Ticket, Name, EndpointId (newtype), Chat aggregate root with business rules (e.g. validate message length, nonce uniqueness)                                                               |
| domain/error.rs             | DomainError enum with variants like InvalidTicket, MessageTooLong, PeerNotFound. Implements std::error::Error + Display. No strings.                                                                |
| src/application/            | Use cases / application services. Orchestrates domain + ports. Depends only on domain + ports.                                                                                                      |
| application/chat_service.rs | ChatService struct holding Arcdyn NetworkPort. Methods: open(), join(), send(), next_message(). Contains the rolling history for LLM if enabled.                                                    |
| application/llm_relay.rs    | LlmRelayService that takes LlmPort + history and produces replies (streaming or batched). Decides when to send partials.                                                                            |
| application/dto.rs          | Data Transfer Objects: ChatMessageDto { id, from, text }, OpenResult { ticket, endpoint_id } — clean boundaries for adapters.                                                                       |
| src/ports/                  | Trait definitions only. The contracts between layers.                                                                                                                                               |
| ports/network_port.rs       | pub trait NetworkPort: Send + Sync { async fn broadcast(&self, msg: Message) - Result(); async fn subscribe(...) }                                                                                  |
| ports/llm_port.rs           | pub trait LlmPort: Send + Sync { async fn stream_reply(&self, history: &HistoryMessage) - Resultimpl StreamItem=Delta; }                                                                            |
| src/adapters/               | All concrete implementations. Only place that depends on iroh, reqwest, uniffi, data_encoding, etc. Split into driver (inbound) and driven (outbound).                                              |
| adapters/driven/iroh/       | IrohGossipAdapter { endpoint, gossip, router } that implements NetworkPort. Contains the subscribe_loop, AboutMe logic, Ticket serialization.                                                       |
| adapters/driven/openai/     | OpenAiLlmAdapter that implements LlmPort using reqwest + the existing SSE parser (moved here).                                                                                                      |
| adapters/driver/uniffi/     | UniffiChatAdapter (the uniffi::Object Chat). Holds ArcChatService and ArcLlmRelayService. Thin delegation + error mapping. This is the only file that uses uniffi::Object / Record / Error derives. |
| src/infrastructure/         | Cross-cutting concerns + wiring.                                                                                                                                                                    |
| infrastructure/di.rs        | pub fn build_chat_service(config: &Config) - ResultChatService { let network = IrohGossipAdapter::new(...); ChatService::new(network) }                                                             |
| infrastructure/config.rs    | Config struct with llm_base_url, model, context_size, stream settings, iroh presets, etc. Loaded from env or CLI args.                                                                              |
| src/main.rs (binary)        | Thin CLI entrypoint. Parses args with clap, builds Config, calls infrastructure::build_chat_service, then runs the input + incoming loops using the service. No business logic here.                |
| src/error.rs                | AppError enum that wraps DomainError + AdapterError (thiserror). Maps to user-facing messages. Used by all layers.                                                                                  |


Architecture Diagram (Text)

+-------------------+

 Driver Adapters  (inbound)

 uniffi / cli 

+---------+---------+



v

+------------------+ +------------------+ +------------------+

 Application - Ports - Domain 

 ChatService   NetworkPort   Message, Ticket 

 LlmRelayService   LlmPort   DomainError 

+------------------+ +------------------+ +------------------+

^



+---------+---------+

 Driven Adapters  (outbound)

 iroh / openai 

+-------------------+

Key Code Examples (Skeletons)

1 Domain Entity (domain/chat.rs)

use serde::{Deserialize, Serialize}; derive(Debug, Clone, Serialize,
Deserialize) pub struct Message { pub body: MessageBody, pub nonce:
u8; 16, } derive(Debug, Clone, Serialize, Deserialize) pub
enum MessageBody { AboutMe { from: EndpointId, name: Name }, Text {
from: EndpointId, text: String }, } impl Message { pub fn new_text(from:
EndpointId, text: String) - ResultSelf, DomainError { if
text.len()  4096 { return Err(DomainError::MessageTooLong); } Ok(Self
{ body: MessageBody::Text { from, text }, nonce: rand::random() }) } }

2 Outbound Port (ports/network_port.rs)

use async_trait::async_trait; use crate::domain::{Message, Ticket,
EndpointId}; async_trait pub trait NetworkPort: Send + Sync {
async fn open(&self) - ResultTicket, AppError; async fn
join(&self, ticket: Ticket) - ResultEndpointId, AppError; async fn
broadcast(&self, msg: Message) - Result(), AppError; // Returns a
stream or channel of incoming messages async fn subscribe(&self) -
Resultmpsc::Receiverdomain::ChatMessageDto, AppError; }

3 Application Service (application/chat_service.rs)

pub struct ChatService { network: Arcdyn NetworkPort, llm:
OptionArcdyn LlmPort, // ... history, config } impl ChatService
{ pub async fn send(&self, text: String) - Result(), AppError {
let msg = Message::new_text(self.my_id, text)?;
self.network.broadcast(msg).await?; // also echo to local subscribers
Ok(()) } pub async fn enable_llm_relay(&mut self, llm: Arcdyn
LlmPort) { self.llm = Some(llm); } }

4 Uniffi Driver Adapter (adapters/driver/uniffi/ffi_chat.rs)

derive(uniffi::Object) pub struct Chat { service:
ArcChatService, } uniffi::export(async_runtime = "tokio") impl
Chat { uniffi::constructor pub fn new(name: OptionString) -
Self { let service =
infrastructure::build_chat_service(&Config::default()); Self { service:
Arc::new(service) } } pub async fn send(&self, text: String) -
Result(), ChatError {
self.service.send(text).await.map_err(map_to_chat_error) } pub async fn
next_message(&self) - OptionChatMessage { ... } }

Migration Steps from Current Spike

1. **Extract Domain:** Move Message, MessageBody, Ticket, Name,
  EndpointId (newtype wrapper around iroh::EndpointId) into domain/.
    Remove all serde derives if they are only for wire (keep for now,
    but consider separate DTOs). Make constructors enforce invariants.
2. **Define Ports:** Create ports/network_port.rs with the trait. Move
  the iroh-specific subscribe logic signature into the trait. Same for
    LlmPort (extract the generate_reply contract).
3. **Create Application Layer:** ChatService becomes the home for the
  current open/join/send/next_message logic + history management. The
    LLM relay moves into its own service that ChatService can optionally
    compose.
4. **Move Adapters:** iroh code →
  adapters/driven/iroh/gossip_adapter.rs (impl NetworkPort).
    OpenAI/SSE code → adapters/driven/openai/llm_adapter.rs (impl
    LlmPort). The entire current lib.rs Chat object →
    adapters/driver/uniffi/ffi_chat.rs (thin wrapper).
5. **Wire Everything:** Add infrastructure/di.rs with a build function
  that creates the concrete adapters and injects them into
    ChatService. Update main.rs to use the new factory instead of
    Chat::new directly.
6. **Update uniffi:** Keep uniffi::setup_scaffolding!() in lib.rs. The
  FFI types (ChatError, ChatMessage) stay in the uniffi adapter or a
    shared ffi_types module.
7. **Add Tests:** Write unit tests in application/ that use mockall or
  manual mocks for NetworkPort. Integration tests in tests/ that spin
    up real IrohGossipAdapter.

Recommended Additional Crates (Minimal)


|                    |                                                                                               |
| ------------------ | --------------------------------------------------------------------------------------------- |
| **Crate**          | **Purpose & Where Used**                                                                      |
| thiserror          | DomainError + AppError with source and from. Replaces manual Display impls.                   |
| async-trait        | For async fn in trait (NetworkPort, LlmPort).                                                 |
| tokio              | Already used; keep for mpsc, spawn, etc. Feature "rt-multi-thread", "macros".                 |
| mockall (dev-dep)  | For mocking ports in application unit tests.                                                  |
| serde + serde_json | Keep for wire format in domain (or move to adapter if you want domain to be format-agnostic). |


Final Advice & Next Steps

Start small: Extract domain/ first (1-2 days), then ports/ (half day),
then application/ (the service logic), then move the iroh code into
adapters/driven/. The uniffi adapter can stay almost identical in
behavior while the internals are cleaned up.

Keep the current spike code in a branch or examples/ directory as the
"reference implementation" until the new structure passes the same
manual tests (open → join → send → LLM reply).

**This layout positions guyos-core to become a proper open-source
library** that multiple platforms can consume, with a clear contribution
guide: "Add a new domain rule? → domain/. New P2P backend? →
adapters/driven/your-backend/". The hexagonal boundary makes the
project attractive for contributors who only want to touch iroh or only
want to improve the Swift binding.

*The spike proved the concept. Hexagonal architecture will prove the
longevity.*