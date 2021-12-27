use std::{
    collections::HashSet,
    env,
    fs::{remove_file, OpenOptions},
    io::{BufReader, Write},
};

use serde::{Deserialize, Serialize};
use twilight_http::Client;
use twilight_model::{
    channel::{message::Message, Channel, ChannelType},
    id::{ChannelId, GuildId, MessageId},
};

const STATE_FILE: &'static str = ".discord_scrape_state";
const MESSAGE_CHUNK_SIZE: u64 = 100;

#[derive(Serialize, Deserialize, Debug)]
struct State {
    current_guild: GuildId,
    current_channel: Option<ChannelId>,
    last_message: Option<MessageId>,
    channels_complete: HashSet<ChannelId>,
}

fn get_active_state() -> std::io::Result<State> {
    let file = OpenOptions::new().read(true).open(STATE_FILE)?;
    let reader = BufReader::new(file);

    Ok(simd_json::from_reader(reader).expect("Unable to parse state file"))
}

fn save_active_state(state: &State) -> std::io::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(STATE_FILE)?;

    simd_json::to_writer(file, state).expect("Unable to serialize state");

    Ok(())
}

async fn fetch_message_chunk(
    client: &Client,
    state: &State,
) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    let req = client
        .channel_messages(
            state
                .current_channel
                .expect("Fetched message chunk without channel... wot?"),
        )
        .limit(MESSAGE_CHUNK_SIZE)?;

    eprintln!(
        "Fetching message chunk {:?}/{:?}",
        state.current_channel, state.last_message
    );
    Ok(if let Some(last_message) = state.last_message {
        req.before(last_message).exec().await?.models().await?
    } else {
        req.exec().await?.models().await?
    })
}

async fn fetch_archived_threads(
    client: &Client,
    channel: &ChannelId,
    channels: &mut Vec<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
    channels.append(
        &mut client
            .public_archived_threads(*channel)
            .exec()
            .await?
            .model()
            .await?
            .threads,
    );
    channels.append(
        &mut client
            .private_archived_threads(*channel)
            .exec()
            .await?
            .model()
            .await?
            .threads,
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bot_token = env::var("BOT_TOKEN")?;
    let guild_id =
        GuildId::new(u64::from_str_radix(&env::var("GUILD_ID")?, 10)?).expect("Invalid guild ID");

    let client = Client::new(format!("Bot {}", bot_token));

    let mut state = get_active_state().unwrap_or_else(|_| State {
        current_guild: guild_id,
        current_channel: None,
        last_message: None,
        channels_complete: HashSet::new(),
    });

    assert_eq!(guild_id, state.current_guild);

    save_active_state(&state)?;

    let mut channels: Vec<Channel> = Vec::new();

    eprintln!("Fetching channels...");
    channels.extend(
        client
            .guild_channels(state.current_guild)
            .exec()
            .await?
            .models()
            .await?
            .iter()
            .map(|c| Channel::Guild(c.to_owned())),
    );

    eprintln!("Fetching active threads...");
    channels.append(
        &mut client
            .active_threads(state.current_guild)
            .exec()
            .await?
            .model()
            .await?
            .threads,
    );

    let mut messages: Vec<Message> = Vec::new();
    let mut counter = 0;

    eprintln!("Fetching messages...");
    while let Some(channel) = channels.pop() {
        if channel.kind() != ChannelType::GuildText
            && channel.kind() != ChannelType::GuildPublicThread
            && channel.kind() != ChannelType::GuildPrivateThread
        {
            eprintln!("Skipping {} (bad type {:?})", channel.id(), channel.kind());
            continue;
        }

        // Skip channels we've already read
        if state.channels_complete.contains(&channel.id()) {
            eprintln!("Skipping {} (already done)", channel.id());
            continue;
        }

        // Fetch archived threads
        if channel.kind() == ChannelType::GuildText {
            if let Err(e) = fetch_archived_threads(&client, &channel.id(), &mut channels).await {
                eprintln!("Error fetching archived threads {:?}", e);
            }
        }

        let file_name = format!("{}.messages.json", channel.id());
        let mut file = if state.current_channel != Some(channel.id()) {
            state.current_channel = Some(channel.id());
            state.last_message = None;

            let meta_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(format!("{}.meta.json", channel.id()))?;
            simd_json::to_writer(meta_file, &channel)?;

            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(file_name)?;
            write!(file, "[")?;
            file
        } else {
            OpenOptions::new().write(true).open(file_name)?
        };
        save_active_state(&state)?;

        while state.last_message.is_none()
            || messages.len().try_into().unwrap_or(0) == MESSAGE_CHUNK_SIZE
        {
            match fetch_message_chunk(&client, &state).await {
                Ok(r) => messages = r,
                Err(e) => {
                    eprintln!("Error getting message chunk {:?}", e);
                    break;
                }
            }

            eprintln!(
                "Received message chunk {}/{}",
                messages.len(),
                MESSAGE_CHUNK_SIZE
            );

            let message_count = messages.len();
            if message_count == 0 {
                break;
            }

            if state.last_message.is_some() {
                write!(file, ",")?;
            }
            for i in 0..message_count {
                simd_json::to_writer(&mut file, &messages[i])?;
                if i < message_count - 1 {
                    write!(file, ",")?;
                }
            }

            state.last_message = messages.last().map(|m| m.id);
            save_active_state(&state)?;
        }

        write!(file, "]")?;
        state.channels_complete.insert(channel.id());
        save_active_state(&state)?;

        counter += 1;
        eprintln!(
            "[{}/{:?}] Completed channel {}...",
            counter,
            channels.len() + state.channels_complete.len(),
            channel.id()
        );
    }

    remove_file(STATE_FILE)?;
    eprintln!("Done!");

    Ok(())
}
