use super::onvif_clients::{get_snapshot_uris, OnvifClients};
use crate::utils::create_onvif_user_and_fix_snapshot_uri;
use anyhow::bail;
use log::{error, warn};
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
    clients: Option<OnvifClients>,
    event_subscription: Option<SoapClient>,
}

impl OnvifCamera {
    pub async fn new(uri: &str, username: &str, password: &str) -> Result<Self, String> {
        let credentials: Credentials = Credentials {
            username: username.to_string(),
            password: password.to_string(),
        };
        let clients = match OnvifClients::new(uri, Some(username), Some(password)).await {
            Ok(cli) => Some(cli),
            Err(err) => {
                error!("cannot create OnvifCamera clients. err:{}", err);
                None
            }
        };
        Ok(Self {
            uri: uri.to_string(),
            credentials,
            event_subscription: None,
            clients,
        })
    }

    pub async fn init(&mut self) {
        if let Some(clients) = &self.clients {
            if let Some(event) = &clients.event {
                self.event_subscription = match self.create_event_pull_message_client(event).await {
                    Ok(pull_client) => Some(pull_client),
                    Err(err) => {
                        error!("cannot create pull client. err:{}", err);
                        None
                    }
                }
            } else {
                error!("cannot create event subscription for camera:{}", self.uri);
            }
        } else {
            let clients = match OnvifClients::new(
                &self.uri,
                Some(&self.credentials.username),
                Some(&self.credentials.password),
            )
            .await
            {
                Ok(cli) => Some(cli),
                Err(err) => {
                    warn!("cannot create OnvifCamera clients. err:{}", err);
                    None
                }
            };
            self.clients = clients;
        }
    }

    pub async fn get_snapshot_uri(&self) -> anyhow::Result<String> {
        if let Some(clients) = &self.clients {
            if let Some(media) = &clients.media {
                // get onvif snapshot uris
                match get_snapshot_uris(media).await {
                    Ok(snapshot_uris) => {
                        for snapshot_uri in snapshot_uris {
                            match download_picture(&snapshot_uri).await {
                                Ok(_) => return Ok(snapshot_uri),
                                Err(_) => {
                                    // if onvif uri doesnt work, create new user, replace credentials and try again
                                    match create_onvif_user_and_fix_snapshot_uri(
                                        &self.uri,
                                        &snapshot_uri,
                                    )
                                    .await
                                    {
                                        Ok(fixed_url) => {
                                            if download_picture(&fixed_url).await.is_ok() {
                                                return Ok(fixed_url);
                                            }
                                        }
                                        Err(err) => bail!("{}", err),
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
        bail!("no clients initilialized for camera: {}", self.uri)
    }

    pub async fn get_event_message(&mut self) -> anyhow::Result<PullMessagesResponse> {
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
                Err(err) => {
                    self.init().await;
                    bail!("cannot get message: {}", err);
                }
            }
        } else {
            self.init().await;
        }
        bail!("client not registered");
    }

    pub fn connected(&self) -> bool {
        self.clients.is_some()
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

        // println!(
        //     "camera pull point subscription termination: {:?}",
        //     camera_sub.termination_time
        // );

        let uri: Url = Url::parse(&camera_sub.subscription_reference.address).unwrap();
        Ok(ClientBuilder::new(&uri).build())
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
