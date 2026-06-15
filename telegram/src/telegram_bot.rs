use super::bot_command::{command_handler, BotCommand};
use app_core::traits::notifier::Notifier;
use repository::memory_repository::MemoryRepository;
use std::sync::Arc;
use teloxide::{
    dispatching::Dispatcher,
    dispatching::{HandlerExt, UpdateFilterExt},
    dptree,
    types::Update,
    Bot,
};

#[derive(Clone)]
pub struct TelegramBot {
    client: Bot,
    allowed_chat_ids: Vec<String>,
    repository: Arc<MemoryRepository>,
    notifier: Arc<dyn Notifier>,
}

impl TelegramBot {
    pub fn new(
        bot_token: String,
        chat_ids: Vec<String>,
        repository: Arc<MemoryRepository>,
        notifier: Arc<dyn Notifier>,
    ) -> Self {
        Self {
            client: Bot::new(bot_token),
            allowed_chat_ids: chat_ids,
            repository,
            notifier,
        }
    }

    pub async fn start(&self) {
        let handler = Update::filter_message().branch(
            dptree::entry()
                .filter_command::<BotCommand>()
                .endpoint(command_handler),
        );

        Dispatcher::builder(self.client.clone(), handler)
            .dependencies(dptree::deps![
                self.allowed_chat_ids.clone(),
                self.repository.clone(),
                self.notifier.clone(),
                self.clone()
            ])
            .build()
            .dispatch()
            .await;
    }
}
