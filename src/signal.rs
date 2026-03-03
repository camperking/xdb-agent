use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;


use futures::{channel::oneshot, future, pin_mut, StreamExt};
use presage::libsignal_service::configuration::SignalServers;
use presage::libsignal_service::content::{ContentBody, DataMessage};
use presage::model::identity::OnNewIdentity;
use presage::model::messages::Received;
use presage::Manager;
use presage_store_sqlite::{SqliteConnectOptions, SqliteStore};
use rig::completion::{Chat, Message};
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

/// Run the Signal chat daemon.
///
/// Receives messages from Signal, forwards text messages from whitelisted senders
/// and groups to the LLM agent, and sends the agent's response back via Signal.
pub async fn run_daemon<A>(db_path: &str, config: &Config, agent: Agent<A>) -> anyhow::Result<()>
where
    A: Chat,
{
    let (allowed_uuids, allowed_groups) = parse_allowlists(config);
    let mut conversation_histories: HashMap<ConversationKey, Vec<Message>> = HashMap::new();

    info!(db_path, "loading registered Signal manager");
    let store = open_store(db_path).await?;
    let mut manager = Manager::load_registered(store).await?;

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

    let messages = manager.receive_messages().await?;
    pin_mut!(messages);

    let mut queue_drained = false;

    while let Some(item) = messages.next().await {
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

                let history = conversation_histories
                    .get(&conversation_key)
                    .cloned()
                    .unwrap_or_default();

                info!(?sender, "received: {text}");

                // Forward to agent
                let reply = match agent.chat_with_history(&text, history).await {
                    Ok(response) => {
                        let entry = conversation_histories.entry(conversation_key).or_default();
                        entry.push(Message::user(text.clone()));
                        entry.push(Message::assistant(response.clone()));
                        response
                    }
                    Err(e) => {
                        error!(%e, "agent chat failed");
                        format!("Sorry, an error occurred: {e}")
                    }
                };

                info!(?sender, "replying: {reply}");

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

    warn!("message stream ended");
    Ok(())
}


