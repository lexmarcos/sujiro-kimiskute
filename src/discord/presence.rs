use serenity::all::ActivityData;

use crate::config::{BotActivityConfig, BotActivityType};

pub(super) fn activity_data(configuration: &BotActivityConfig) -> ActivityData {
    let message = configuration.message();
    match configuration.activity_type() {
        BotActivityType::Playing => ActivityData::playing(message),
        BotActivityType::Watching => ActivityData::watching(message),
        BotActivityType::Listening => ActivityData::listening(message),
        BotActivityType::Competing => ActivityData::competing(message),
    }
}
