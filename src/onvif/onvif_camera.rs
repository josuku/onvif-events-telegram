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
    pub snapshot_uri: Option<String>,
    pub credentials: Credentials,
    clients: OnvifClients,
    event_subscription: Option<SoapClient>,
}

impl OnvifCamera {
    pub async fn new(
        uri: &str,
        username: &str,
        password: &str,
        snapshot_uri: Option<String>,
    ) -> Result<Self, String> {
        let credentials: Credentials = Credentials {
            username: username.to_string(),
            password: password.to_string(),
        };
        let clients = OnvifClients::new(uri, Some(username), Some(password)).await?;

        Ok(Self {
            uri: uri.to_string(),
            snapshot_uri,
            credentials,
            event_subscription: None,
            clients,
        })
    }

    pub async fn get_snapshot(&self) -> anyhow::Result<Vec<u8>> {
        if let Some(uri) = &self.snapshot_uri {
            let response = match reqwest::get(uri.clone()).await {
                Ok(resp) => resp,
                Err(_) => bail!("Failed to download image"),
            };

            let image = match response.bytes().await {
                Ok(bytes) => bytes.to_vec(),
                Err(_) => bail!("Failed to get bytes of image"),
            };

            Ok(image)
        } else {
            bail!("camera {} doesnt have snapshot uri", self.uri);
        }
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

        if self.snapshot_uri.is_none() {
            if let Some(media) = &self.clients.media {
                match get_snapshot_uris(media).await {
                    Ok(uris) => {
                        for uri in uris {
                            if check_uri_ok(&uri).await {
                                self.snapshot_uri = Some(uri.clone());
                                break;
                            }
                        }
                        // TODO if no uri works, create new onvif user and replace in uri
                    }
                    Err(err) => error!("cannot get snapshot: {}", err),
                }
            }
        } else {
            println!("snapshot_uri already exists for camera {}", self.uri);
        }
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

async fn check_uri_ok(uri: &str) -> bool {
    match reqwest::get(uri).await {
        Ok(_) => true,
        Err(err) => {
            error!("cannot access uri:{} err:{}", uri, err);
            false
        }
    }
}
