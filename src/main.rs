// This example pulls messages related to the RuleEngine topic.
// RuleEngine topic consists of events related to motion detection.
// Tested on Dahua, uniview, reolink and axis ip cameras.
// Don't forget to set the camera's IP address, username and password.

use onvif::soap::client::{ClientBuilder, Credentials};
use rustygram::types::{SendMessageOption, SendMessageParseMode};
use schema::{
    b_2::NotificationMessageHolderType,
    event::{self, CreatePullPointSubscription, PullMessages},
};
use url::Url;

#[derive(Debug, Clone)]
pub struct Camera {
    pub device_service_url: String,
    pub username: String,
    pub password: String,
    pub event_service_url: String,
    pub snapshot_url: String,
}

const BOT_TOKEN: &str = "xxx";  // paste your bot token
const CHAT_ID: &str = "xxx";    // paste your chat id
const CAMERA_IP: &str = "192.168.xxx.xxx:8899"; // paste your camera IP with port
const USERNAME: &str = "user";  // paste your camera user
const PASSWORD: &str = "pass";  // paste your camera password

#[tokio::main]
async fn main() {
    let rust_connector = rustygram::create_bot(BOT_TOKEN, CHAT_ID);

    let camera: Camera = Camera {
        device_service_url: format!("http://{}/onvif/device_service", CAMERA_IP),
        username: USERNAME.to_string(),
        password: PASSWORD.to_string(),
        event_service_url: format!("http://{}/onvif/event_service", CAMERA_IP),
        snapshot_url: format!(
            "http://{}/webcapture.jpg?command=snap&channel=0&user={}&password={}",
            CAMERA_IP, USERNAME, PASSWORD
        ),
    };

    let creds: Credentials = Credentials {
        username: camera.username.to_string(),
        password: camera.password.to_string(),
    };
    let event_client = ClientBuilder::new(&Url::parse(&camera.event_service_url).unwrap())
        .credentials(Some(creds))
        .build();
    let create_pull_sub_request = CreatePullPointSubscription {
        initial_termination_time: None,
        // filter: Some(b_2::FilterType {
        //     topic_expression: Some(b_2::TopicExpressionType {
        //         dialect: "http://www.onvif.org/ver10/tev/topicExpression/ConcreteSet".to_string(),
        //         inner_text: "tns1:RuleEngine//.".to_string(),
        //     }),
        // }),
        filter: None,
        subscription_policy: None,
    };
    let create_pull_puint_sub_response =
        event::create_pull_point_subscription(&event_client, &create_pull_sub_request).await;
    let camera_sub = match create_pull_puint_sub_response {
        Ok(sub) => sub,
        Err(e) => {
            println!("Error: {:?}", e);
            return;
        }
    };

    let uri: Url = Url::parse(&camera_sub.subscription_reference.address).unwrap();
    let creds: Credentials = Credentials {
        username: camera.username.to_string(),
        password: camera.password.to_string(),
    };
    let pull_msg_client = ClientBuilder::new(&uri)
        .credentials(Some(creds))
        .auth_type(onvif::soap::client::AuthType::Digest)
        .build();
    let pull_messages_request = PullMessages {
        message_limit: 256,
        timeout: xsd_types::types::Duration {
            seconds: 1.0,
            ..Default::default()
        },
    };

    // Main Loop
    loop {
        let pull_messages_response =
            event::pull_messages(&pull_msg_client, &pull_messages_request).await;
        let msg = match pull_messages_response {
            Ok(msg) => msg,
            Err(e) => {
                println!("Error: {:?}", e);
                continue;
            }
        };
        if !msg.notification_message.is_empty() {
            let option = SendMessageOption {
                parse_mode: Some(SendMessageParseMode::HTML),
            };

            let human_detections = msg
                .notification_message
                .iter()
                .filter(|msg| {
                    msg.message
                        .msg
                        .source
                        .simple_item
                        .iter()
                        .any(|si| si.name == "Rule" && si.value == "MyMotionDetectorRule")
                        == true
                        && msg
                            .message
                            .msg
                            .data
                            .simple_item
                            .iter()
                            .any(|si| si.name == "IsMotion" && si.value == "true")
                            == true
                })
                .collect::<Vec<&NotificationMessageHolderType>>();

            if !human_detections.is_empty() {
                let message_to_send = format!(
                    r#"
<u>New detection</u>
<em>Start: {}</em>
<em>End: {}</em>"#,
                    msg.current_time, msg.termination_time
                );
                if let Err(err) =
                    rustygram::send_message(&rust_connector, message_to_send.as_str(), Some(option))
                        .await
                {
                    println!("cannot send message to Telegram {:?}", err)
                }

                let response = reqwest::get(camera.snapshot_url.clone())
                    .await
                    .expect("Failed to download image");
                let image = response
                    .bytes()
                    .await
                    .expect("Failed to get bytes of image");

                    let caption = format!(
                        r#"
New detection
Start: {}
End: {}"#,
                        msg.current_time, msg.termination_time
                    );

                if let Err(err) =
                    rustygram::send_picture(&rust_connector, image.to_vec(), "file_name.jpg", &caption)
                        .await
                {
                    println!("cannot send picture to Telegram {:?}", err)
                }
                println!("{:?}", image);
            }
            println!("Notification Message: {:?}", msg);
        } else {
            println!("No new notification message");
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    }
}
