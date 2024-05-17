use bible::scripture::bible::Bible;
use commands::*;
use helpers::print_color::PrintCommand;
use helpers::response_builder::ResponseBuilder;
use helpers::statics::{
    get_running_time, initialize_statics, lookup_command_prefix, update_command_prefix,
    CHANNELS_PER_LISTENER, DEFAULT_TRANSLATION, METRICS, REPLY_CHARACTER_LIMIT,
    START_DATETIME_LOCAL_STRING, START_DATETIME_UTC_STRING, TWITCH_ACCOUNT,
};
use helpers::Metrics;
use tokio::sync::mpsc;

use futures::future::pending;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;
use twitch::chat::{client::WebSocketState, Listener, Replier};
use twitch::common::message_data::{MessageData, Type};

use helpers::config::Config;
use helpers::env_variables::get_env_variable;
use helpers::statics::{avaialble_bibles, find_bible};
use helpers::statics::{BIBLES, CHANNELS_TO_JOIN};

mod commands;
mod helpers;

#[tokio::main]
async fn main() {
    initialize_statics();
    PrintCommand::System.print_message("ChapterVerse", "Jesus is Lord!");
    PrintCommand::Issue.print_message("Version", env!("CARGO_PKG_VERSION"));
    PrintCommand::Issue.print_message("Start UTC", &START_DATETIME_UTC_STRING);
    PrintCommand::Issue.print_message("Start Local", &START_DATETIME_LOCAL_STRING);
    PrintCommand::Info.print_message("What is the Gospel?", "Gospel means good news! The bad news is we have all sinned and deserve the wrath to come. But Jesus the Messiah died for our sins, was buried, and then raised on the third day, according to the scriptures. He ascended into heaven and right now is seated at the Father's right hand. Jesus said, \"I am the way, and the truth, and the life. No one comes to the Father except through me. The time is fulfilled, and the kingdom of God is at hand; repent and believe in the gospel.\"");
    for (bible_name, bible_arc) in BIBLES.iter() {
        let bible: &Bible = &*bible_arc; // Dereference the Arc and immediately borrow the result
        let scripture = match bible.get_scripture("2 Timothy 3:16") {
            verses if !verses.is_empty() => {
                let scriptures = verses
                    .iter()
                    .map(|verse| verse.scripture.clone())
                    .collect::<Vec<_>>()
                    .join(" ");
                scriptures
            }
            _ => "Verse not found".to_string(),
        };

        PrintCommand::Info.print_message(&format!("{}, 2 Timothy 3:16", bible_name), &scripture);
    }
    let twitch_oauth = get_env_variable("TWITCHOAUTH", "oauth:1234567890abcdefghijklmnopqrst");
    let replier = Arc::new(Replier::new(&TWITCH_ACCOUNT, &twitch_oauth));

    let (listener_transmitter, mut listener_reciever) = mpsc::unbounded_channel::<MessageData>();
    let (replier_transmitter, mut replier_receiver) = mpsc::unbounded_channel::<MessageData>();
    let listeners = Arc::new(Mutex::new(HashMap::<String, Arc<Listener>>::new()));
    let replier_transmitter_clone = Arc::new(Listener::new(replier_transmitter.clone()));
    let listeners_clone = Arc::clone(&listeners);
    let listener_transmitter_clone = listener_transmitter.clone();

    // **Spawn a task to Listens for incoming Twitch messages.
    tokio::spawn(async move {
        while let Some(mut message) = listener_reciever.recv().await {
            // let tags = message.tags.clone();

            if !message.tags.contains(&Type::Ignore) {
                let channel = &message.channel;
                let prefix = lookup_command_prefix(channel);
                let mut message_text_lowercase = message.text.to_lowercase();

                if message_text_lowercase.starts_with(prefix) {
                    message_text_lowercase.replace_range(0..1, "!");
                    message.tags.push(Type::PossibleCommand);
                } else if message_text_lowercase.contains("gospel message") {
                    message.tags.push(Type::Gospel);
                } else if message_text_lowercase.contains(":") {
                    message.tags.push(Type::PossibleScripture);
                } else {
                    message.tags.push(Type::None);
                }

                let mut reply: Option<String> = None;
                let display_name = message.display_name.unwrap();
                let message_text = message.text.to_string();

                for tag in message.tags.clone() {
                    match tag {
                        Type::None => (),
                        Type::Gospel => {
                            message.tags.push(Type::Gospel);
                            Metrics::add_user(&METRICS, &display_name).await;
                            Metrics::increment_gospels_english(&METRICS).await;
                            reply = gospel(&display_name);
                        }
                        Type::PossibleCommand => {
                            let mut parts = message_text_lowercase.split_whitespace();
                            let command = parts.next().unwrap_or_default().to_string();
                            let params: Vec<String> = parts.map(|s| s.to_string()).collect();

                            reply = match command.as_str() {
                                "!help" => {
                                    message.tags.push(Type::Command);
                                    Metrics::add_user(&METRICS, &display_name).await;
                                    help(avaialble_bibles, &prefix)
                                }
                                "!joinchannel" => {
                                    message.tags.push(Type::Command);
                                    message.tags.push(Type::ExcludeMetrics);

                                    let mut config = Config::load(&display_name);

                                    Metrics::add_user_and_channel(&METRICS, &display_name).await;

                                    if !config.channel.as_ref().unwrap().active.unwrap_or(false) {
                                        config.join_channel(&channel);
                                        let new_twitch_listener = Arc::new(Listener::new(
                                            listener_transmitter_clone.clone(),
                                        ));
                                        match new_twitch_listener.clone().connect().await {
                                            Ok(_) => {
                                                // println!("Successfully connected. - Not Actually - it is in process");
                                                let _ = new_twitch_listener
                                                    .clone()
                                                    .join_channel(display_name)
                                                    .await;
                                            }
                                            Err(e) => {
                                                eprintln!("Failed to connect: {:?}", e);
                                                tokio::time::sleep(
                                                    tokio::time::Duration::from_secs(5),
                                                )
                                                .await;
                                                continue;
                                            }
                                        }
                                        let listeners_lock = listeners_clone.lock();
                                        listeners_lock
                                            .await
                                            .insert(display_name.to_string(), new_twitch_listener);
                                        Some(
                                            format!(
                                                "Praise God, we have a new user of ChapterVerse, {}! ChapterVerse has joined your channel, type !help for list of available commands. Isaiah 55:1 - So shall My word be that goes forth from My mouth; It shall not return to Me void But it shall accomplish what I please And it shall prosper in the thing for which I sent it.",
                                                message.display_name.unwrap_or_default()
                                            )
                                            .to_string(),
                                        )
                                    } else {
                                        Some(
                                            format!(
                                                "Already joined {} from {} on : {}",
                                                message.display_name.unwrap_or_default(),
                                                config.get_from_channel(),
                                                config.join_date()
                                            )
                                            .to_string(),
                                        )
                                    }
                                }
                                "!translation" => {
                                    message.tags.push(Type::Command);
                                    Metrics::add_user(&METRICS, &display_name).await;
                                    translation(&display_name, params, avaialble_bibles).await
                                }
                                "!votd" => {
                                    message.tags.push(Type::Command);
                                    message.tags.push(Type::ExcludeMetrics);
                                    Metrics::add_user(&METRICS, &display_name).await;
                                    votd(&channel, &display_name, params).await
                                }
                                "!random" => {
                                    message.tags.push(Type::Command);
                                    Metrics::add_user(&METRICS, &display_name).await;
                                    random(&display_name, params).await
                                }
                                "!next" => {
                                    message.tags.push(Type::Command);
                                    Metrics::add_user(&METRICS, &display_name).await;

                                    match next(&display_name, params).await {
                                        Some(value) => {
                                            Metrics::increment_total_scriptures(&METRICS).await;
                                            message.tags.push(Type::Scripture);
                                            Some(value)
                                        }
                                        None => {
                                            message.tags.push(Type::NotScripture);
                                            None
                                        }
                                    }
                                }
                                "!previous" => {
                                    message.tags.push(Type::Command);
                                    Metrics::add_user(&METRICS, &display_name).await;

                                    match previous(&display_name, params).await {
                                        Some(value) => {
                                            Metrics::increment_total_scriptures(&METRICS).await;
                                            message.tags.push(Type::Scripture);
                                            Some(value)
                                        }
                                        None => {
                                            message.tags.push(Type::NotScripture);
                                            None
                                        }
                                    }
                                }
                                "!leavechannel" => {
                                    message.tags.push(Type::Command);
                                    message.tags.push(Type::ExcludeMetrics);
                                    let mut config = Config::load(&display_name);
                                    config.leave_channel();
                                    Metrics::add_user(&METRICS, &display_name).await;
                                    Metrics::remove_channel(&METRICS, &display_name).await;
                                    let listeners_lock = listeners_clone.lock();
                                    for (_key, listener) in listeners_lock.await.iter() {
                                        match listener.leave_channel(&display_name).await {
                                            Ok(_) => (),
                                            Err(e) => eprintln!(
                                                "Error leaving channel {}: {}",
                                                display_name, e
                                            ),
                                        }
                                    }
                                    Some(
                                        format!(
                                            "ChapterVerse has left the {} channel.",
                                            &display_name
                                        )
                                        .to_string(),
                                    )
                                }
                                "!myinfo" => {
                                    message.tags.push(Type::Command);
                                    message.tags.push(Type::ExcludeMetrics);
                                    Metrics::add_user(&METRICS, &display_name).await;

                                    match myinfo(&display_name, params).await {
                                        Some(value) => Some(value),
                                        None => None,
                                    }
                                }
                                "!support" => {
                                    message.tags.push(Type::Command);
                                    Metrics::add_user(&METRICS, &display_name).await;
                                    support()
                                }
                                "!status" => Some({
                                    message.tags.push(Type::Command);
                                    // ChapterVerse: v3.06 | Totals: 157 channels; 9,100 users; 122,613 scriptures; 12,692 Gospel proclamations! | Current Metrics: 22:0:10:35 uptime, 566,784 messages parsed (0.107ms avg), 4,587 responses (9.271ms avg)
                                    Metrics::add_user(&METRICS, &display_name).await;
                                    let mut metrics = METRICS.write().await;
                                    let metric_channels = metrics.channels.unwrap_or(0).to_string();
                                    let metric_users = metrics.users.unwrap_or(0).to_string();
                                    let metric_scriptures =
                                        metrics.scriptures.unwrap_or(0).to_string();
                                    let metric_gospels = (metrics.gospels_english.unwrap_or(0)
                                        + metrics.gospels_spanish.unwrap_or(0)
                                        + metrics.gospels_german.unwrap_or(0))
                                    .to_string();
                                    let running_time = get_running_time();
                                    let (total_messages_parsed, average_parse_time) =
                                        metrics.message_parsed_stats();
                                    let (total_responses, average_response_time) =
                                        metrics.message_response_stats();

                                    format!(
                                        "v{}, | Totals: {} channels, {} users, {} scriptures, {} Gospel Proclamations! | Daily Metrics: {} uptime, {} messages parsed ({}ms avg), {} responses ({}ms avg)",
                                        env!("CARGO_PKG_VERSION"),
                                        metric_channels,
                                        metric_users,
                                        metric_scriptures,
                                        metric_gospels,
                                        running_time,
                                        total_messages_parsed,
                                        average_parse_time,
                                        total_responses,
                                        average_response_time,
                                    )
                                }),
                                "!commandprefix" => {
                                    message.tags.push(Type::Command);
                                    message.tags.push(Type::ExcludeMetrics);
                                    Metrics::add_user(&METRICS, &display_name).await;

                                    match commandprefix(&display_name, params).await {
                                        (Some(message), prefix) => {
                                            update_command_prefix(
                                                &display_name.to_string(),
                                                &prefix,
                                            );
                                            Some(message)
                                        }
                                        (None, _) => None,
                                    }
                                }
                                "!setvotd" => Some("Set the verse of the day.".to_string()),
                                "!gospel" => {
                                    message.tags.push(Type::Gospel);
                                    Metrics::add_user(&METRICS, &display_name).await;
                                    Metrics::increment_gospels_english(&METRICS).await;
                                    gospel(&display_name)
                                }
                                "!evangelio" => {
                                    message.tags.push(Type::Gospel);
                                    Metrics::add_user(&METRICS, &display_name).await;
                                    Metrics::increment_gospels_spanish(&METRICS).await;
                                    evangelio(&display_name)
                                }
                                "!evangelium" => {
                                    message.tags.push(Type::Gospel);
                                    Metrics::add_user(&METRICS, &display_name).await;
                                    Metrics::increment_gospels_german(&METRICS).await;
                                    evangelium(&display_name)
                                }
                                _ => {
                                    // TODO - might be a scripture so possibly check it against that function.
                                    message.tags.push(Type::NotCommand);
                                    None
                                }
                            };
                        }
                        Type::PossibleScripture => {
                            let mut config = Config::load(&display_name);
                            let perferred_translation = config
                                .get_translation()
                                .unwrap_or_else(|| DEFAULT_TRANSLATION.to_string());

                            let bible_name_to_use =
                                find_bible(message_text.to_string(), &perferred_translation);
                            config.last_translation(&bible_name_to_use);

                            if let Some(bible_arc) = BIBLES.get(&bible_name_to_use) {
                                let bible: &Bible = &*bible_arc;
                                reply = {
                                    let verses = bible.get_scripture(&message.text);
                                    if verses.is_empty() {
                                        message.tags.push(Type::NotScripture);
                                        None
                                    } else {
                                        //@TwitchAccountName + 1 extra space because the name is included in the text that can't exceed 500.
                                        let adjusted_character_limit = *REPLY_CHARACTER_LIMIT
                                            - (message.display_name.unwrap().len() + 1);
                                        let response_output = ResponseBuilder::build(
                                            &verses,
                                            adjusted_character_limit,
                                            &bible_name_to_use,
                                        );
                                        config.set_last_verse(&verses.last().unwrap().reference);
                                        config.add_account_metrics_scriptures();
                                        Metrics::add_user(&METRICS, &display_name).await;
                                        Metrics::increment_total_scriptures(&METRICS).await;
                                        message.tags.push(Type::Scripture);
                                        Some(response_output.truncated)
                                    }
                                };
                                PrintCommand::Info.print_message(
                                    &format!(
                                        "Bible {}, {:?}",
                                        bible_name_to_use, message.display_name
                                    ),
                                    format!("{:?}", reply).as_str(),
                                );
                            } else {
                                eprintln!("Bible named '{}' not found.", bible_name_to_use);
                            }
                        }
                        _ => {
                            {
                                //Handle other message types here if needed
                            }
                        }
                    }
                    match reply {
                        Some(ref reply_value) => {
                            let mut metrics = METRICS.write().await;
                            let duration = message.complete().unwrap_or_default();
                            if !message.tags.contains(&Type::ExcludeMetrics) {
                                metrics.message_response(duration);
                            }

                            message.reply = Some(format!("{} ({}ms)", reply_value, duration));
                            if let Err(e) =
                                { replier_transmitter_clone.message_tx.send(message.clone()) }
                            {
                                eprintln!("Failed to send message: {}", e);
                            }

                            let mut echo_message = message.clone();
                            echo_message.channel = TWITCH_ACCOUNT.to_string();
                            echo_message.reply = Some(format!(
                                "http://twitch.tv/{} {} \"{}\" : {}",
                                &channel,
                                message.display_name.as_ref().map(|s| s).unwrap_or(&""),
                                &message.text,
                                message.reply.as_ref().map(|s| s.as_str()).unwrap_or("")
                            ));
                            echo_message.id = None;
                            if let Err(e) =
                                { replier_transmitter_clone.message_tx.send(echo_message) }
                            {
                                eprintln!("Failed to send message: {}", e);
                            }
                        }
                        None => {
                            // println!("Tages: {:?}", message.tags);
                            let mut metrics = METRICS.write().await;
                            let duration = message.complete().unwrap_or_default();
                            metrics.message_parsed(duration);
                        }
                    }
                }
            }
        }
    });

    let listeners_clone = Arc::clone(&listeners);
    let listener_transmitter_clone = listener_transmitter.clone();
    // Spawn a task to manage connections, listeners, and reconnection
    tokio::spawn(async move {
        loop {
            let new_twitch_listener = Arc::new(Listener::new(listener_transmitter_clone.clone()));
            match new_twitch_listener.clone().connect().await {
                Ok(_) => (),
                Err(e) => {
                    eprintln!("Failed to connect: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    continue;
                }
            }
            for chunk in CHANNELS_TO_JOIN.chunks(*CHANNELS_PER_LISTENER) {
                let chunk_twitch_listener =
                    Arc::new(Listener::new(listener_transmitter_clone.clone()));
                let listeners_lock = listeners_clone.lock();
                listeners_lock.await.insert(
                    chunk_twitch_listener.username.to_string(),
                    chunk_twitch_listener.clone(),
                );
                match chunk_twitch_listener.clone().connect().await {
                    Ok(_) => (),
                    Err(e) => {
                        eprintln!("Failed to connect: {:?}", e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        continue;
                    }
                }
                tokio::spawn({
                    async move {
                        for channel in chunk {
                            let twitch_listener_clone = Arc::clone(&chunk_twitch_listener);
                            match twitch_listener_clone.join_channel(channel).await {
                                Ok(_) => {
                                    Metrics::add_channel(&METRICS, channel).await;
                                }
                                Err(e) => eprintln!("Failed to join channel {}: {}", channel, e),
                            }
                        }
                    }
                });
            }
            while new_twitch_listener.clone().get_state() != WebSocketState::Disconnected {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
    });

    // Spawn a task for replying to messages.
    let replier_clone = Arc::clone(&replier);
    let loop_replier_clone = Arc::clone(&replier);

    tokio::spawn(async move {
        match replier_clone.clone().connect().await {
            Ok(_) => {
                // println!("Successfully connected for Replying.");
                let _ = replier_clone
                    .clone()
                    .send_message(&TWITCH_ACCOUNT, "Jesus is Lord!")
                    .await;
                let _ = replier_clone
                    .clone()
                    .send_message(
                        &TWITCH_ACCOUNT,
                        format!(
                            "ChapterVerse Version: {} | ONLINE: {}",
                            env!("CARGO_PKG_VERSION"),
                            *START_DATETIME_LOCAL_STRING,
                        )
                        .as_str(),
                    )
                    .await;

                // // Test Loop to send 100 messages with a counter and the current time.
                // for count in 1..=10 {
                //     if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
                //         let timestamp = now.as_secs(); // Seconds since UNIX epoch
                //         let message = format!("Debug Count: {} - Timestamp: {}", count, timestamp);
                //         let _ = replier_clone
                //             .clone()
                //             .send_message("TESTACCOUNT", &message)
                //             .await;
                //     }
                // }
            }
            Err(e) => {
                eprintln!("Failed to connect: {:?}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
        while let Some(message) = replier_receiver.recv().await {
            // TODO!  Find out about if I can remove these clones.
            let _ = loop_replier_clone
                .clone()
                // TODO! Update MessageData with a reply_text field
                .reply_message(message)
                .await;
        }
    });

    // This line will keep the program running indefinitely until it's killed manually (e.g., Ctrl+C).
    pending::<()>().await;
}

#[cfg(test)]
mod unittests {
    use super::*;
    // use the following command line to see the results of the test: cargo test -- --nocapture
    #[test]
    fn get_scripture() {
        for (bible_name, bible_arc) in BIBLES.iter() {
            let bible: &Bible = &*bible_arc; // Here you dereference the Arc and immediately borrow the result

            let message = match bible.get_scripture("2 Timothy 3:16") {
                Some(verse) => format!("{}", verse.scripture),
                None => "Verse not found".to_string(),
            };

            PrintCommand::Info.print_message(&format!("{}, 2 Timothy 3:16", bible_name), &message);
        }
    }
}
