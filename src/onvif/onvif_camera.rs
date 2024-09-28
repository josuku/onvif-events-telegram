use crate::{
    onvif::onvif_clients::{DEFAULT_PASSWORD, DEFAULT_USERNAME},
    telegram::telegram_client::replace_snapshot_uri_credentials,
};

use super::onvif_clients::{get_snapshot_uris, OnvifClients};
use anyhow::bail;
use log::error;
use onvif::soap::client::{Client as SoapClient, ClientBuilder, Credentials};
use schema::{
    b_2::NotificationMessageHolderType,
    event::{self, CreatePullPointSubscription, PullMessages, PullMessagesResponse},
};
use url::Url;

#[derive(Clone)]
pub struct OnvifCamera {
    pub uri: String,
    pub credentials: Credentials,
    clients: OnvifClients,
    event_subscription: Option<SoapClient>,
}

impl OnvifCamera {
    pub async fn new(uri: &str, username: &str, password: &str) -> Result<Self, String> {
        let credentials: Credentials = Credentials {
            username: username.to_string(),
            password: password.to_string(),
        };
        let clients = OnvifClients::new(uri, Some(username), Some(password)).await?;

        Ok(Self {
            uri: uri.to_string(),
            credentials,
            event_subscription: None,
            clients,
        })
    }

    pub async fn init(&mut self) {
        if let Some(event) = &self.clients.event {
            let pull_client = self
                .create_event_pull_message_client(event)
                .await
                .expect("cannot create pull client");
            self.event_subscription = Some(pull_client);
        } else {
            error!("cannot create event subscription for camera:{}", self.uri);
        }
    }

    pub async fn get_snapshot_uri(&self) -> anyhow::Result<String> {
        if let Some(media) = &self.clients.media {
            // get onvif snapshot uris
            match get_snapshot_uris(media).await {
                Ok(uris) => {
                    for uri in uris {
                        match download_picture(&uri).await {
                            Ok(_) => return Ok(uri),
                            Err(_) => {
                                // if onvif uri doesnt work, replace credentials and try again
                                let fixed_snapshot_uri = replace_snapshot_uri_credentials(
                                    &uri,
                                    DEFAULT_USERNAME,
                                    DEFAULT_PASSWORD,
                                );
                                if download_picture(&fixed_snapshot_uri).await.is_ok() {
                                    return Ok(fixed_snapshot_uri);
                                }
                            }
                        }
                    }
                    bail!("cannot download picture");
                }
                Err(err) => bail!("cannot get snapshot: {}", err),
            }
        }
        bail!("media client not initilialized for camera: {}", self.uri)
    }

    pub async fn get_event_message(&self) -> anyhow::Result<PullMessagesResponse> {
        if let Some(client) = &self.event_subscription {
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

    async fn create_event_pull_message_client(
        &self,
        event_client: &SoapClient,
    ) -> anyhow::Result<SoapClient> {
        let request = CreatePullPointSubscription {
            initial_termination_time: None,
            filter: None,
            subscription_policy: None,
        };

        let response = event::create_pull_point_subscription(event_client, &request).await;

        let camera_sub = match response {
            Ok(sub) => sub,
            Err(err) => {
                bail!("cannot create pull point subscription:{}", err);
            }
        };

        let uri: Url = Url::parse(&camera_sub.subscription_reference.address).unwrap();
        Ok(ClientBuilder::new(&uri)
            // .credentials(Some(self.credentials.clone()))
            // .auth_type(onvif::soap::client::AuthType::Digest)
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

pub async fn download_picture(uri: &str) -> anyhow::Result<Vec<u8>> {
    let response = match reqwest::get(uri).await {
        Ok(resp) => {
            if resp.status() != 200 {
                bail!("Failed to download image. status:{}", resp.status());
            }
            resp
        }
        Err(_) => bail!("Failed to download image"),
    };
    let image = match response.bytes().await {
        Ok(bytes) => bytes.to_vec(),
        Err(_) => bail!("Failed to get bytes of image"),
    };
    Ok(image)
}
