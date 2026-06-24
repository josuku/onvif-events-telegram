use super::onvif_service_clients::{get_snapshot_uris, OnvifServiceClients};
use crate::onvif_service_clients::{
    create_default_user, get_users, DEFAULT_PASSWORD, DEFAULT_USERNAME,
};
use anyhow::bail;
use app_core::{
    domain::camera::{CameraConnectionData, OnvifCameraEvent},
    traits::camera_client::CameraClient,
};
use async_trait::async_trait;
use diqwest::WithDigestAuth;
use onvif::soap::client::{Client as SoapClient, ClientBuilder};
use schema::{
    b_2::NotificationMessageHolderType,
    event::{self, CreatePullPointSubscription, PullMessages, PullMessagesResponse},
};
use tracing::error;
use url::Url;

pub struct OnvifCameraClient {
    conn_data: CameraConnectionData,
    clients: Option<OnvifServiceClients>,
    event_subscription: Option<SoapClient>,
    snapshot_uri: Option<String>,
    snapshot_requires_auth: bool,
}

impl OnvifCameraClient {
    pub async fn new(uri: &str, username: &str, password: &str) -> Result<Self, String> {
        let conn_data = CameraConnectionData {
            uri: uri.to_string(),
            username: username.to_string(),
            password: password.to_string(),
        };

        Ok(Self {
            conn_data: conn_data.clone(),
            event_subscription: None,
            clients: create_onvif_clients(&conn_data).await,
            snapshot_uri: None,
            snapshot_requires_auth: false,
        })
    }

    pub async fn init(&mut self) {
        if self.clients.is_none() {
            self.clients = create_onvif_clients(&self.conn_data).await;
        }

        if let Some(clients) = &self.clients {
            if let Some(event) = &clients.event {
                self.event_subscription = match create_event_pull_message_client(event).await {
                    Ok(pull_client) => Some(pull_client),
                    Err(err) => {
                        error!("cannot create pull client. err:{}", err);
                        None
                    }
                }
            } else {
                error!(
                    "cannot create event subscription for camera:{}",
                    self.conn_data.uri
                );
            }
        }

        match self.resolve_snapshot_uri().await {
            Ok((uri, requires_auth)) => {
                self.snapshot_uri = Some(uri);
                self.snapshot_requires_auth = requires_auth;
            }
            Err(err) => {
                error!(
                    "cannot get snapshot uri for camera:{}. err:{}",
                    self.conn_data.uri, err
                );
            }
        }
    }

    async fn resolve_snapshot_uri(&self) -> anyhow::Result<(String, bool)> {
        if let Some(clients) = &self.clients {
            if let Some(media) = &clients.media {
                match get_snapshot_uris(media).await {
                    Ok(snapshot_uris) => {
                        let creds = Some((
                            self.conn_data.username.as_str(),
                            self.conn_data.password.as_str(),
                        ));
                        for snapshot_uri in snapshot_uris {
                            match download_picture_from_uri(&snapshot_uri, None).await {
                                Ok(_) => return Ok((snapshot_uri, false)),
                                Err(_) => {
                                    // try with auth before giving up on this URI
                                    if download_picture_from_uri(&snapshot_uri, creds)
                                        .await
                                        .is_ok()
                                    {
                                        return Ok((snapshot_uri, true));
                                    }
                                    error!("cannot download picture from uri. trying to create new onvif user.");
                                    match self
                                        .create_user_and_fix_snapshot_uri(
                                            &self.conn_data.uri,
                                            &snapshot_uri,
                                        )
                                        .await
                                    {
                                        Ok(fixed_url) => {
                                            if download_picture_from_uri(&fixed_url, None)
                                                .await
                                                .is_ok()
                                            {
                                                return Ok((fixed_url, false));
                                            }
                                            if download_picture_from_uri(&fixed_url, creds)
                                                .await
                                                .is_ok()
                                            {
                                                return Ok((fixed_url, true));
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
            bail!(
                "media client not initialized for camera: {}",
                self.conn_data.uri
            )
        }
        bail!("no clients initialized for camera: {}", self.conn_data.uri)
    }
}

#[async_trait]
impl CameraClient for OnvifCameraClient {
    async fn snapshot(&self) -> anyhow::Result<Vec<u8>> {
        if let Some(snapshot_uri) = &self.snapshot_uri {
            let creds = if self.snapshot_requires_auth {
                Some((
                    self.conn_data.username.as_str(),
                    self.conn_data.password.as_str(),
                ))
            } else {
                None
            };
            download_picture_from_uri(snapshot_uri, creds).await
        } else {
            bail!("cannot download picture from camera:{}", self.conn_data.uri)
        }
    }

    async fn get_snapshot_uri(&self) -> anyhow::Result<String> {
        self.resolve_snapshot_uri().await.map(|(uri, _)| uri)
    }

    async fn get_event_message(&self) -> anyhow::Result<Option<OnvifCameraEvent>> {
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
                Ok(msg) => {
                    if is_motion_detection(&msg) {
                        return Ok(Some(OnvifCameraEvent {
                            r#type: app_core::domain::camera::CameraEventType::Motion,
                            timestamp: msg.current_time.value.to_utc(),
                        }));
                    } else {
                        return Ok(None);
                    }
                }
                Err(err) => {
                    // self.init().await;
                    bail!("cannot get message. error:{}", err);
                }
            }
        } else {
            // self.init().await;
            bail!("no event subscription to get event message");
        }
    }

    fn connected(&self) -> bool {
        self.clients.is_some()
    }

    fn get_connection_data(&self) -> CameraConnectionData {
        self.conn_data.clone()
    }

    async fn create_user_and_fix_snapshot_uri(
        &self,
        camera_uri: &str,
        orig_snapshot_uri: &str,
    ) -> anyhow::Result<String> {
        match get_users(camera_uri).await {
            Ok(users) => {
                if !users.contains(&DEFAULT_USERNAME.to_string()) {
                    if let Err(err) = create_default_user(camera_uri).await {
                        anyhow::bail!("cannot create user {}. error:{}", DEFAULT_USERNAME, err);
                    }
                }
                Ok(replace_snapshot_uri_credentials(
                    orig_snapshot_uri,
                    DEFAULT_USERNAME,
                    DEFAULT_PASSWORD,
                ))
            }
            Err(_) => {
                anyhow::bail!("cannot get users of camera:{}", camera_uri)
            }
        }
    }
}

pub async fn create_onvif_camera_client(
    uri: &str,
    username: &str,
    password: &str,
) -> anyhow::Result<OnvifCameraClient> {
    let mut client = match OnvifCameraClient::new(uri, username, password).await {
        Ok(cli) => cli,
        Err(err) => {
            bail!(
                "cannot create OnvifCamera in url:{} with user:{}. error:{}",
                uri,
                username,
                err
            );
        }
    };
    client.init().await;
    Ok(client)
}

async fn create_onvif_clients(conn_data: &CameraConnectionData) -> Option<OnvifServiceClients> {
    match OnvifServiceClients::new(
        &conn_data.uri,
        Some(&conn_data.username),
        Some(&conn_data.password),
    )
    .await
    {
        Ok(cli) => Some(cli),
        Err(err) => {
            error!("cannot create OnvifCamera clients. err:{}", err);
            None
        }
    }
}

async fn create_event_pull_message_client(event_client: &SoapClient) -> anyhow::Result<SoapClient> {
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

    // debug!(
    //     "camera pull point subscription termination: {:?}",
    //     camera_sub.termination_time
    // );

    let uri: Url = Url::parse(&camera_sub.subscription_reference.address).unwrap();
    Ok(ClientBuilder::new(&uri).build())
}

// TODO add other detection types like tamper, etc...
fn is_motion_detection(msg: &PullMessagesResponse) -> bool {
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

async fn download_picture_from_uri(
    uri: &str,
    credentials: Option<(&str, &str)>,
) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::new();

    let response = match credentials {
        None => client
            .get(uri)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to download image: {}", e))?,
        Some((username, password)) => client
            .get(uri)
            .send_digest_auth((username, password))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to download image with digest auth: {}", e))?,
    };

    if response.status() != 200 {
        bail!("Failed to download image. status:{}", response.status());
    }

    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|_| anyhow::anyhow!("Failed to get bytes of image"))
}

fn replace_snapshot_uri_credentials(
    snapshot_uri: &str,
    new_user: &str,
    new_password: &str,
) -> String {
    // Case 1 - for URLS with this format
    // http://192.168.1.217/webcapture.jpg?command=snap&channel=0&user=yfyf&password=aZlg5hk1
    let mut url = Url::parse(snapshot_uri).unwrap();
    let mut query_pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    for pair in query_pairs.iter_mut() {
        match pair.0.as_str() {
            "user" => pair.1 = new_user.to_string(),
            "password" => pair.1 = new_password.to_string(),
            _ => {}
        }
    }
    let new_query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(query_pairs)
        .finish();
    url.set_query(Some(&new_query));
    url.to_string()
}
