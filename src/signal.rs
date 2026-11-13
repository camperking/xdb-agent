use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;
use std::future::Future;


use futures::{channel::oneshot, future, pin_mut, StreamExt};
use tokio::sync::mpsc;
use presage::libsignal_service::configuration::SignalServers;
use presage::libsignal_service::content::{ContentBody, DataMessage};
use presage::model::identity::OnNewIdentity;
use presage::model::messages::Received;
use presage::store::Thread;
use presage::Manager;
use presage_store_sqlite::{SqliteConnectOptions, SqliteStore};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::agent::Agent;
use crate::config::Config;

type GroupMasterKeyBytes = [u8; 32];

/// Open (or create) the SQLite store at the given path.
async fn open_store(db_path: &str) -> anyhow::Result<SqliteStore> {
    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Use open_with_options so we can set create_if_missing(true);
    // SqliteStore::open() does not set this flag and fails on a fresh install.
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true);
    let store = SqliteStore::open_with_options(options, OnNewIdentity::Trust).await?;
    Ok(store)
}

/// Link this device as a secondary Signal device.
/// Displays a QR code in the terminal for the user to scan with their primary Signal app.
pub async fn link_device(db_path: &str, device_name: &str) -> anyhow::Result<()> {
    info!(db_path, device_name, "linking as secondary device");

    let store = open_store(db_path).await?;

    let (provisioning_link_tx, provisioning_link_rx) = oneshot::channel();

    let (manager_result, _) = future::join(
        Manager::link_secondary_device(
            store,
            SignalServers::Production,
            device_name.into(),
            provisioning_link_tx,
        ),
        async move {
            match provisioning_link_rx.await {
                Ok(url) => {
                    println!("\nPlease scan this QR code with your Signal app:\n");
                    qr2term::print_qr(url.to_string()).expect("failed to render QR code");
                    println!("\nAlternatively, open this URL: {url}\n");
                }
                Err(e) => error!(%e, "linking was cancelled"),
            }
        },
    )
    .await;

    match manager_result {
        Ok(manager) => {
            let whoami = manager.whoami().await?;
            println!("Successfully linked! Identity: {whoami:?}");
        }
        Err(e) => {
            anyhow::bail!("Failed to link device: {e}");
        }
    }

    Ok(())
}

/// Return the current UNIX timestamp in milliseconds (used for Signal message timestamps).
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_millis() as u64
}

/// Parse the config's allowed UUIDs and group master keys into usable types.
fn parse_allowlists(config: &Config) -> (Vec<Uuid>, Vec<GroupMasterKeyBytes>) {
    let allowed_uuids: Vec<Uuid> = config
        .allowed_uuids
        .iter()
        .filter_map(|s| match s.parse::<Uuid>() {
            Ok(u) => Some(u),
            Err(e) => {
                warn!(uuid = %s, %e, "ignoring invalid UUID in config");
                None
            }
        })
        .collect();

    let allowed_groups: Vec<GroupMasterKeyBytes> = config
        .allowed_groups
        .iter()
        .filter_map(|s| {
            match hex::decode(s) {
                Ok(bytes) if bytes.len() == 32 => {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&bytes);
                    Some(key)
                }
                Ok(bytes) => {
                    warn!(group = %s, len = bytes.len(), "ignoring group key with wrong length (expected 32 bytes)");
                    None
                }
                Err(e) => {
                    warn!(group = %s, %e, "ignoring invalid hex group key in config");
                    None
                }
            }
        })
        .collect();

    (allowed_uuids, allowed_groups)
}

/// Which thread a reply should be sent to.
enum AllowedThread {
    Contact,
    Group(GroupMasterKeyBytes),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ConversationKey {
    Contact(Uuid),
    Group(GroupMasterKeyBytes),
}

fn short_owner_label(key: &ConversationKey) -> String {
    match key {
        ConversationKey::Contact(uuid) => {
            let s = uuid.to_string();
            let short = s.get(..8).unwrap_or(&s);
            format!("contact:{short}")
        }
        ConversationKey::Group(master_key) => {
            let hex_key = hex::encode(master_key);
            let short = hex_key.get(..12).unwrap_or(&hex_key);
            format!("group:{short}")
        }
    }
}

fn format_agent_input(text: &str, sender_name: Option<&str>) -> String {
    match sender_name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => format!("[sender={name}]\n{text}"),
        None => text.to_string(),
    }
}

fn should_suppress_reply(reply: &str) -> bool {
    reply.trim() == "HEARTBEAT_OK"
}

fn has_mention(text: &str) -> bool {
    text.to_lowercase().contains("xdevbot")
}

fn is_slash_command(text: &str) -> bool {
    let content = text
        .lines()
        .find(|line| !line.starts_with("[sender="))
        .unwrap_or(text);
    content.trim().starts_with('/')
}

fn strip_mention(text: &str) -> String {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    let mention = "xdevbot";
    if lower.starts_with(mention) {
        trimmed[mention.len()..].trim_start().to_string()
    } else if lower.ends_with(mention) {
        trimmed[..trimmed.len() - mention.len()].trim_end().to_string()
    } else {
        text.to_string()
    }
}

/// Check if a message is from an allowed source.
fn check_allowed(
    sender: &Uuid,
    data_message: &DataMessage,
    allowed_uuids: &[Uuid],
    allowed_groups: &[GroupMasterKeyBytes],
) -> Option<AllowedThread> {
    // Check if this is a group message
    if let Some(ref group_v2) = data_message.group_v2 {
        if let Some(ref master_key_bytes) = group_v2.master_key {
            if master_key_bytes.len() == 32 {
                println!("GROUP KEY: {}", hex::encode(master_key_bytes));
                let mut key = [0u8; 32];
                key.copy_from_slice(master_key_bytes);
                if allowed_groups.contains(&key) {
                    return Some(AllowedThread::Group(key));
                }
                debug!(group = hex::encode(key), "group not in allowlist");
                return None;
            }
        }
    }

    // Direct message — check sender UUID
    if allowed_uuids.contains(sender) {
        return Some(AllowedThread::Contact);
    }

    debug!(?sender, "contact not in allowlist");
    None
}

/// Initial delay before reconnecting after a WebSocket disconnection.
const RECONNECT_INITIAL_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
/// Maximum delay between reconnection attempts (caps exponential backoff).
const RECONNECT_MAX_DELAY: std::time::Duration = std::time::Duration::from_secs(60);

/// Run the Signal chat daemon.
///
/// Receives messages from Signal, forwards text messages from whitelisted senders
/// and groups to the LLM agent, and sends the agent's response back via Signal.
///
/// Automatically reconnects when the WebSocket stream ends (e.g. idle timeout,
/// transient network errors) using exponential backoff.
pub async fn run_daemon<F, Fut>(db_path: &str, config: &Config, make_agent: F) -> anyhow::Result<()>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Agent>,
{
    let (allowed_uuids, allowed_groups) = parse_allowlists(config);
    let mut conversation_agents: HashMap<ConversationKey, Agent> = HashMap::new();

    info!("Signal daemon running — waiting for messages…");
    if allowed_uuids.is_empty() && allowed_groups.is_empty() {
        warn!("no allowed UUIDs or groups configured — all messages will be ignored");
    } else {
        info!(
            uuids = ?allowed_uuids,
            groups = allowed_groups.len(),
            "responding to whitelisted contacts and groups"
        );
    }

    let mut reconnect_delay = RECONNECT_INITIAL_DELAY;

    loop {
        info!(db_path, "loading registered Signal manager");
        let store = match open_store(db_path).await {
            Ok(s) => s,
            Err(e) => {
                error!(%e, "failed to open store, retrying in {:?}", reconnect_delay);
                tokio::time::sleep(reconnect_delay).await;
                reconnect_delay = (reconnect_delay * 2).min(RECONNECT_MAX_DELAY);
                continue;
            }
        };

        let mut manager = match Manager::load_registered(store).await {
            Ok(m) => m,
            Err(e) => {
                error!(%e, "failed to load registered manager, retrying in {:?}", reconnect_delay);
                tokio::time::sleep(reconnect_delay).await;
                reconnect_delay = (reconnect_delay * 2).min(RECONNECT_MAX_DELAY);
                continue;
            }
        };

        let messages = match manager.receive_messages().await {
            Ok(m) => m,
            Err(e) => {
                error!(%e, "failed to start receiving messages, retrying in {:?}", reconnect_delay);
                tokio::time::sleep(reconnect_delay).await;
                reconnect_delay = (reconnect_delay * 2).min(RECONNECT_MAX_DELAY);
                continue;
            }
        };

        // Reset backoff on successful connection
        reconnect_delay = RECONNECT_INITIAL_DELAY;
        info!("WebSocket connected — listening for messages");

        // Channel to decouple the WebSocket driver from message processing.
        // The receive stream MUST be polled continuously — even while we're
        // busy with LLM calls or sending replies — because presage's duplex
        // WebSocket requires it for sends to make progress.
        let (tx, mut rx) = mpsc::unbounded_channel::<Received>();

        // Task A: continuously poll the receive stream and forward items.
        tokio::task::spawn_local(async move {
            pin_mut!(messages);
            while let Some(item) = messages.next().await {
                if tx.send(item).is_err() {
                    break;
                }
            }
            info!("receive stream ended");
        });

        // Task B: process incoming items (LLM + send replies).
        let mut queue_drained = false;

        while let Some(item) = rx.recv().await {
            match item {
                Received::QueueEmpty => {
                    info!("message queue drained — ready to process new messages");
                    queue_drained = true;
                }
                Received::Contacts => {
                    debug!("contacts synchronized");
                }
                Received::Content(content) => {
                    let sender = content.metadata.sender.raw_uuid();
                    let sender_service_id = content.metadata.sender.clone();

                    // Only handle DataMessage with a text body
                    let data_message = match &content.body {
                        ContentBody::DataMessage(dm) => dm,
                        _ => {
                            debug!(?sender, "ignoring non-data message");
                            continue;
                        }
                    };

                    let text = match &data_message.body {
                        Some(text) => text.clone(),
                        None => {
                            debug!(?sender, "ignoring data message without text body");
                            continue;
                        }
                    };

                    // Check allowlist (contacts + groups)
                    let thread = match check_allowed(&sender, data_message, &allowed_uuids, &allowed_groups) {
                        Some(t) => t,
                        None => continue,
                    };

                    if !queue_drained {
                        debug!(?sender, "skipping queued message received before queue drained");
                        continue;
                    }

                    let conversation_key = match &thread {
                        AllowedThread::Contact => ConversationKey::Contact(sender),
                        AllowedThread::Group(master_key) => ConversationKey::Group(*master_key),
                    };
                    let owner_label = short_owner_label(&conversation_key);

                    if !conversation_agents.contains_key(&conversation_key) {
                        let mut agent = make_agent().await;
                        agent.set_owner(owner_label.clone());
                        conversation_agents.insert(conversation_key.clone(), agent);
                    }

                    let conversation_agent = conversation_agents
                        .get_mut(&conversation_key)
                        .expect("conversation agent must exist after insertion");

                    let sender_name = match manager
                        .thread_title(&Thread::Contact(sender_service_id.clone()))
                        .await
                    {
                        Ok(title) => {
                            let sender_id = sender_service_id.service_id_string();
                            if title.trim().is_empty() || title == sender_id {
                                None
                            } else {
                                Some(title)
                            }
                        }
                        Err(e) => {
                            debug!(%e, ?sender, "failed to resolve sender display name");
                            None
                        }
                    };

                    let agent_input = format_agent_input(&text, sender_name.as_deref());

                    info!(
                        owner = %owner_label,
                        ?sender,
                        sender_name = sender_name.as_deref().unwrap_or("<none>"),
                        "received: {text}"
                    );
                    debug!(
                        owner = %owner_label,
                        ?sender,
                        sender_name = sender_name.as_deref().unwrap_or("<none>"),
                        "agent input: {agent_input}"
                    );

                    // In group chats, require @Xdevbot mention (slash commands bypass this)
                    if matches!(thread, AllowedThread::Group(_))
                        && !has_mention(&text)
                        && !is_slash_command(&text)
                    {
                        debug!(owner = %owner_label, ?sender, "adding group message to history without mention");
                        conversation_agent.add_to_history(&agent_input);
                        continue;
                    }

                    // Strip @Xdevbot mention from start/end of group messages
                    let agent_input = if matches!(thread, AllowedThread::Group(_)) {
                        format_agent_input(&strip_mention(&text), sender_name.as_deref())
                    } else {
                        agent_input
                    };

                    // Forward to conversation-specific agent (history owned by the agent)
                    let reply = match conversation_agent.chat(&agent_input).await {
                        Ok(response) => response,
                        Err(e) => {
                            error!(%e, "agent chat failed");
                            format!("Sorry, an error occurred: {e}")
                        }
                    };

                    if should_suppress_reply(&reply) {
                        debug!(owner = %owner_label, ?sender, "suppressing HEARTBEAT_OK reply");
                        continue;
                    }

                    info!(owner = %owner_label, ?sender, "replying: {reply}");

                    // Send reply to the appropriate thread
                    match thread {
                        AllowedThread::Contact => {
                            let data_message = DataMessage {
                                body: Some(reply),
                                ..Default::default()
                            };
                            if let Err(e) = manager
                                .send_message(sender_service_id, data_message, now_ms())
                                .await
                            {
                                error!(%e, ?sender, "failed to send reply");
                            }
                        }
                        AllowedThread::Group(master_key) => {
                            let data_message = DataMessage {
                                body: Some(reply),
                                group_v2: Some(presage::libsignal_service::content::GroupContextV2 {
                                    master_key: Some(master_key.to_vec()),
                                    revision: Some(0),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            };
                            if let Err(e) = manager
                                .send_message_to_group(&master_key, data_message, now_ms())
                                .await
                            {
                                error!(%e, group = hex::encode(master_key), "failed to send group reply");
                            }
                        }
                    }
                }
            }
        }

        // Channel closed — the receive task ended (WebSocket disconnected).
        warn!("message stream ended (WebSocket disconnected), reconnecting in {:?}", reconnect_delay);
        tokio::time::sleep(reconnect_delay).await;
        reconnect_delay = (reconnect_delay * 2).min(RECONNECT_MAX_DELAY);
    }
}


