use anyhow::Result;
use clap::Parser;
use guyos_core::Chat;
use qrcode::render::unicode;
use qrcode::QrCode;
use std::collections::VecDeque;
use std::time::Duration;
use tokio::task::JoinHandle;

/// Chat over iroh-gossip
///
/// This broadcasts unsigned messages over iroh-gossip.
///
/// By default a new endpoint id is created when starting the example.
///
/// By default, we use the default n0 discovery services to dial by `EndpointId`.
#[derive(Parser, Debug)]
struct Args {
    /// Set your nickname.
    #[clap(short, long)]
    name: Option<String>,

    /// Enable relaying chat messages to a local OpenAI-compatible LLM.
    #[clap(long, default_value_t = false)]
    llm_enable: bool,

    /// Base URL for the OpenAI-compatible API (should include /v1).
    #[clap(long, default_value = "http://127.0.0.1:11434/v1")]
    llm_base_url: String,

    /// Model name to use (required when --llm-enable).
    #[clap(long)]
    llm_model: Option<String>,

    /// Optional system prompt to prepend.
    #[clap(long)]
    llm_system_prompt: Option<String>,

    /// Number of recent chat messages to include as context.
    #[clap(long, default_value_t = 20)]
    llm_context: usize,

    /// Minimum number of new characters before sending a partial update.
    #[clap(long, default_value_t = 48)]
    llm_stream_chunk_min_chars: usize,

    /// Minimum interval (ms) between partial updates.
    #[clap(long, default_value_t = 350)]
    llm_stream_interval_ms: u64,

    /// Enable streaming responses (SSE). If not set, the LLM reply is sent as one message.
    #[clap(long, default_value_t = false)]
    llm_stream: bool,

    #[clap(subcommand)]
    command: Command,
}

#[derive(Parser, Debug)]
enum Command {
    /// Open a chat room for a topic and print a ticket for others to join.
    Open,
    /// Join a chat room from a ticket.
    Join {
        /// The ticket, as base32 string.
        ticket: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let chat = Chat::new(args.name.clone());

    match &args.command {
        Command::Open => {
            println!("> opening chat room...");
            let ticket = chat.open().await?;
            println!("> ticket to join us: {ticket}");
            println!();
            println!("> or scan:");
            print!("{}", render_ticket_qr(&ticket)?);
            println!("> waiting for endpoints to join us...");
        }
        Command::Join { ticket } => {
            println!("> joining chat room...");
            chat.join(ticket.clone()).await?;
            println!("> connected!");
        }
    }

    // Print incoming messages (pull-based).
    // When LLM relay is enabled, the CLI is the only consumer of next_message(),
    // and it both prints and optionally relays.
    let llm_cfg = if args.llm_enable {
        let assistant_name = args
            .name
            .clone()
            .ok_or_else(|| anyhow::anyhow!("--name is required when --llm-enable is set"))?;
        let model = args
            .llm_model
            .clone()
            .ok_or_else(|| anyhow::anyhow!("--llm-model is required when --llm-enable is set"))?;
        Some(LlmRelayConfig {
            assistant_name,
            base_url: args.llm_base_url.clone(),
            model,
            system_prompt: args.llm_system_prompt.clone(),
            context: args.llm_context,
            stream: args.llm_stream,
            stream_chunk_min_chars: args.llm_stream_chunk_min_chars,
            stream_interval: Duration::from_millis(args.llm_stream_interval_ms),
        })
    } else {
        None
    };

    // spawn an input thread that reads stdin
    // create a multi-provider, single-consumer channel
    let (line_tx, mut line_rx) = tokio::sync::mpsc::channel(1);
    // and pass the `sender` portion to the `input_loop`
    std::thread::spawn(move || input_loop(line_tx));

    // broadcast each line we type
    println!("> type a message and hit enter to broadcast...");

    // Incoming messages + optional relay loop
    let incoming_chat = chat.clone();
    let relay_chat = chat.clone();
    let incoming_loop: JoinHandle<Result<()>> = tokio::spawn(async move {
        let mut history: VecDeque<llm::HistoryMessage> = VecDeque::new();
        let mut current_gen: Option<JoinHandle<()>> = None;

        let llm = llm_cfg.map(|cfg| (cfg, llm::OpenAiCompatClient::new()));

        loop {
            let Some(msg) = incoming_chat.next_message().await else {
                break;
            };

            println!("{}: {}", msg.from, msg.text);

            if let Some((cfg, client)) = &llm {
                // Update rolling history.
                history.push_back(llm::HistoryMessage {
                    from: msg.from.clone(),
                    text: msg.text.clone(),
                });
                while history.len() > cfg.context {
                    history.pop_front();
                }

                // Avoid self-loop.
                if msg.from == cfg.assistant_name {
                    continue;
                }

                // Restart generation on every new user message.
                if let Some(handle) = current_gen.take() {
                    handle.abort();
                }
                let cfg = cfg.clone();
                let client = client.clone();
                let chat_for_send = relay_chat.clone();
                let history_snapshot: Vec<llm::HistoryMessage> = history.iter().cloned().collect();

                current_gen = Some(tokio::spawn(async move {
                    if let Err(err) =
                        llm::run_streaming_reply(&client, &cfg, history_snapshot, chat_for_send)
                            .await
                    {
                        eprintln!("> llm relay error: {err}");
                    }
                }));
            }
        }

        Ok(())
    });

    // listen for lines that we have typed to be sent from `stdin`
    while let Some(text) = line_rx.recv().await {
        chat.send(text.clone()).await?;
        // print to ourselves the text that we sent
        println!("> sent: {text}");
    }

    let _ = incoming_loop.await?;

    Ok(())
}

#[derive(Clone, Debug)]
struct LlmRelayConfig {
    assistant_name: String,
    base_url: String,
    model: String,
    system_prompt: Option<String>,
    context: usize,
    stream: bool,
    stream_chunk_min_chars: usize,
    stream_interval: Duration,
}

mod llm;

fn render_ticket_qr(ticket: &str) -> Result<String> {
    let code = QrCode::new(ticket.as_bytes())?;
    let qr = code
        .render::<unicode::Dense1x2>()
        .quiet_zone(true)
        .build();
    Ok(format!("{qr}\n"))
}

fn input_loop(line_tx: tokio::sync::mpsc::Sender<String>) -> Result<()> {
    let mut buffer = String::new();
    let stdin = std::io::stdin(); // We get `Stdin` here.
    loop {
        stdin.read_line(&mut buffer)?;
        line_tx.blocking_send(buffer.clone())?;
        buffer.clear();
    }
}
