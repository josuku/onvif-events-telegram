use crate::config::CameraConfig;
use anyhow::bail;
use onvif::soap::client::{Client as SoapClient, ClientBuilder, Credentials};
use schema::{
    b_2::NotificationMessageHolderType,
    event::{self, CreatePullPointSubscription, PullMessages, PullMessagesResponse},
};
use url::Url;

pub struct OnvifCameraClient {
    pub camera_name: String,
    ip: String,
    snapshot_uri: String,
    credentials: Credentials,
    pull_client: Option<SoapClient>,
}

impl OnvifCameraClient {
    pub fn new(camera_config: CameraConfig) -> Self {
        let credentials: Credentials = Credentials {
            username: camera_config.username,
            password: camera_config.password,
        };
        Self {
            camera_name: camera_config.name,
            ip: camera_config.ip,
            snapshot_uri: camera_config.snapshot_uri,
            credentials,
            pull_client: None,
        }
    }

    pub async fn init(&mut self) {
        let event_client = self.create_event_client();
        let pull_client = self
            .create_pull_msg_client(event_client)
            .await
            .expect("cannot create pull client");
        self.pull_client = Some(pull_client);
    }

    pub async fn get_pull_message(&self) -> anyhow::Result<PullMessagesResponse> {
        if let Some(client) = &self.pull_client {
            let request = PullMessages {
                message_limit: 256,
                timeout: xsd_types::types::Duration {
                    seconds: 1.0,
                    ..Default::default()
                },
            };

            let pull_messages_response = event::pull_messages(client, &request).await;

            match pull_messages_response {
                Ok(msg) => return Ok(msg),
                Err(err) => bail!("cannot get message: {}", err),
            }
        }
        bail!("client not registered");
    }

    pub async fn get_snapshot(&self) -> anyhow::Result<Vec<u8>> {
        let response = match reqwest::get(self.snapshot_uri.clone()).await {
            Ok(resp) => resp,
            Err(_) => bail!("Failed to download image"),
        };

        let image = match response.bytes().await {
            Ok(bytes) => bytes.to_vec(),
            Err(_) => bail!("Failed to get bytes of image"),
        };

        Ok(image)
    }

    fn create_event_client(&self) -> SoapClient {
        let event_service_url = format!("http://{}/onvif/event_service", self.ip);
        let event_service_url = &Url::parse(&event_service_url).unwrap();
        ClientBuilder::new(event_service_url)
            .credentials(Some(self.credentials.clone()))
            .build()
    }

    async fn create_pull_msg_client(&self, event_client: SoapClient) -> anyhow::Result<SoapClient> {
        let request = CreatePullPointSubscription {
            initial_termination_time: None,
            filter: None,
            subscription_policy: None,
        };

        let response = event::create_pull_point_subscription(&event_client, &request).await;

        let camera_sub = match response {
            Ok(sub) => sub,
            Err(err) => {
                bail!("cannot create pull point subscription:{}", err);
            }
        };

        let uri: Url = Url::parse(&camera_sub.subscription_reference.address).unwrap();
        Ok(ClientBuilder::new(&uri)
            .credentials(Some(self.credentials.clone()))
            .auth_type(onvif::soap::client::AuthType::Digest)
            .build())
    }
}

pub fn is_new_detection(msg: &PullMessagesResponse) -> bool {
    !msg.notification_message.is_empty()
        && !msg
            .notification_message
            .iter()
            .filter(|msg| {
                msg.message
                    .msg
                    .source
                    .simple_item
                    .iter()
                    .any(|si| si.name == "Rule" && si.value == "MyMotionDetectorRule")
                    && msg
                        .message
                        .msg
                        .data
                        .simple_item
                        .iter()
                        .any(|si| si.name == "IsMotion" && si.value == "true")
            })
            .collect::<Vec<&NotificationMessageHolderType>>()
            .is_empty()
}
