use serenity::{all::Command, builder::CreateCommand, http::Http};
use tracing::info;

use super::{leave, pause, play, queue, resume, skip, stop};

pub async fn register_global(http: &Http) -> Result<(), serenity::Error> {
    let commands: Vec<CreateCommand> = definitions();
    let registered = Command::set_global_commands(http, commands).await?;

    info!(
        command_count = registered.len(),
        "global commands registered"
    );
    Ok(())
}

fn definitions() -> Vec<CreateCommand> {
    vec![
        play::definition(),
        pause::definition(),
        resume::definition(),
        skip::definition(),
        stop::definition(),
        queue::definition(),
        leave::definition(),
    ]
}
