use rustygram::bot::Bot;
use xsd_types::types::DateTime;

pub struct TelegramClient {
    client: Bot,
}

impl TelegramClient {
    pub fn new(bot_token: String, chat_id: String) -> Self {
        Self {
            client: rustygram::create_bot(&bot_token, &chat_id),
        }
    }

    pub async fn send_message_with_picture(
        &self,
        time: &DateTime,
        duration: i64,
        camera_name: String,
        picture: Vec<u8>,
    ) {
        let caption = format!(
            r#"
{}
Time: {}
Duration: {}s"#,
            camera_name, time.value, duration
        );

        if let Err(err) =
            rustygram::send_picture(&self.client, picture, "file_name.jpg", &caption).await
        {
            println!("cannot send picture to Telegram {:?}", err)
        }
    }
}
