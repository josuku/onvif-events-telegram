use super::onvif_rs_service_clients::{get_snapshot_uris, OnvifRsServiceClients};
use crate::onvif_rs_service_clients::{
    create_default_user, get_users, DEFAULT_PASSWORD, DEFAULT_USERNAME,
};
use anyhow::bail;
use app_core::{
    domain::camera::{CameraConnectionData, CameraEventType, OnvifCameraEvent},
    traits::onvif_camera_client::OnvifCameraClient,
};
use async_trait::async_trait;
use diqwest::{DigestAuthSession, WithDigestAuth};
use futures_util::lock::Mutex;
use onvif::soap::client::{AuthType, Client as SoapClient, ClientBuilder};
use schema::{
    b_2::NotificationMessageHolderType,
    event::{self, CreatePullPointSubscription, PullMessages, PullMessagesResponse},
    transport::Transport,
};
use std::{str::FromStr, sync::Arc};
use tracing::error;
use url::Url;

pub const PULL_SUBSCRIPTION_TIMEOUT: &str = "PT30M"; // 30 minutes

pub struct OnvifRsCameraClient {
    conn_data: CameraConnectionData,
    clients: Option<OnvifRsServiceClients>,
    event_subscription: Option<SoapClient>,
    snapshot_uri: Option<String>,
    snapshot_requires_auth: bool,
    http_client: reqwest::Client,
    digest_session: Option<Arc<Mutex<DigestAuthSession>>>,
}

impl OnvifRsCameraClient {
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
            http_client: reqwest::Client::new(),
            digest_session: None,
        })
    }

    pub async fn init(&mut self) {
        if self.clients.is_none() {
            self.clients = create_onvif_clients(&self.conn_data).await;
        }

        if let Some(clients) = &self.clients {
            if let Some(event) = &clients.event {
                self.event_subscription =
                    match create_event_pull_message_client(event, &self.conn_data).await {
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
                if requires_auth {
                    self.digest_session = Some(Arc::new(Mutex::new(DigestAuthSession::new(
                        &self.conn_data.username,
                        &self.conn_data.password,
                    ))));
                }
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
                        let creds = (
                            self.conn_data.username.as_str(),
                            self.conn_data.password.as_str(),
                        );
                        for snapshot_uri in snapshot_uris {
                            match download_picture_from_uri(&self.http_client, &snapshot_uri).await
                            {
                                Ok(_) => return Ok((snapshot_uri, false)),
                                Err(_) => {
                                    // try with auth before giving up on this URI
                                    if download_picture_from_uri_with_creds(
                                        &self.http_client,
                                        &snapshot_uri,
                                        creds,
                                    )
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
                                            if download_picture_from_uri(
                                                &self.http_client,
                                                &fixed_url,
                                            )
                                            .await
                                            .is_ok()
                                            {
                                                return Ok((fixed_url, false));
                                            }
                                            if download_picture_from_uri_with_creds(
                                                &self.http_client,
                                                &fixed_url,
                                                creds,
                                            )
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
impl OnvifCameraClient for OnvifRsCameraClient {
    async fn snapshot(&self) -> anyhow::Result<Vec<u8>> {
        if let Some(snapshot_uri) = &self.snapshot_uri {
            let t0 = std::time::Instant::now();
            let result = if let Some(session) = &self.digest_session {
                let session = session.lock().await;
                download_picture_with_session(&self.http_client, snapshot_uri, &session).await
            } else {
                download_picture_from_uri(&self.http_client, snapshot_uri).await
            };
            tracing::debug!(
                "snapshot from {} took {}ms (digest_auth:{})",
                self.conn_data.uri,
                t0.elapsed().as_millis(),
                self.snapshot_requires_auth,
            );
            result
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
                    if let Some(event_type) = parse_event_type(&msg) {
                        return Ok(Some(OnvifCameraEvent {
                            r#type: event_type,
                            timestamp: msg.current_time.value.to_utc(),
                        }));
                    } else {
                        tracing::debug!("unrecognized event:{:?}", msg);
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

    async fn unsubscribe(&self) {
        let Some(sub_client) = &self.event_subscription else {
            return;
        };
        let body = r#"<wsnt:Unsubscribe xmlns:wsnt="http://docs.oasis-open.org/wsn/b-2"/>"#;
        match sub_client.request(body).await {
            Ok(_) => tracing::info!("unsubscribed from camera:{}", self.conn_data.uri),
            Err(err) => tracing::warn!(
                "unsubscribe failed for camera:{} (ignored, subscription will expire): {}",
                self.conn_data.uri,
                err
            ),
        }
    }

    async fn renew_subscription(&self, termination_time: &str) {
        let Some(sub_client) = &self.event_subscription else {
            return;
        };
        let body = format!(
            r#"<wsnt:Renew xmlns:wsnt="http://docs.oasis-open.org/wsn/b-2">
                 <wsnt:TerminationTime>{termination_time}</wsnt:TerminationTime>
               </wsnt:Renew>"#
        );
        match sub_client.request(&body).await {
            Ok(_) => tracing::info!(
                "subscription renewed for camera:{} until +{}",
                self.conn_data.uri,
                termination_time
            ),
            Err(err) => tracing::warn!(
                "subscription renew failed for camera:{}: {}",
                self.conn_data.uri,
                err
            ),
        }
    }
}

pub async fn create_onvif_camera_client(
    uri: &str,
    username: &str,
    password: &str,
) -> anyhow::Result<OnvifRsCameraClient> {
    let mut client = match OnvifRsCameraClient::new(uri, username, password).await {
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

async fn create_onvif_clients(conn_data: &CameraConnectionData) -> Option<OnvifRsServiceClients> {
    match OnvifRsServiceClients::new(
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

async fn create_event_pull_message_client(
    event_client: &SoapClient,
    conn_data: &CameraConnectionData,
) -> anyhow::Result<SoapClient> {
    let initial_termination_time =
        match xsd_types::types::Duration::from_str(PULL_SUBSCRIPTION_TIMEOUT) {
            Ok(duration) => Some(schema::b_2::AbsoluteOrRelativeTimeType::Duration(duration)),
            Err(_) => None,
        };

    let request = CreatePullPointSubscription {
        initial_termination_time,
        filter: None,
        subscription_policy: None,
    };

    let camera_sub = match event::create_pull_point_subscription(event_client, &request).await {
        Ok(sub) => sub,
        Err(_) => {
            // Some cameras (Dahua) require expplicit UsernameToken, retry with it
            let base_uri = Url::parse(&conn_data.uri).unwrap();
            let event_uri = base_uri.join("/onvif/event_service").unwrap();

            tracing::warn!(
                "trying to create again with event_uri:{} with username:{} and Digest",
                event_uri,
                conn_data.username
            );
            let retry_client = ClientBuilder::new(&event_uri)
                .credentials(Some(onvif::soap::client::Credentials {
                    username: conn_data.username.clone(),
                    password: conn_data.password.clone(),
                }))
                .auth_type(AuthType::Digest)
                .build();
            event::create_pull_point_subscription(&retry_client, &request)
                .await
                .map_err(|err| anyhow::anyhow!("cannot create pull point subscription: {}", err))?
        }
    };

    let uri = Url::parse(&camera_sub.subscription_reference.address)
        .map_err(|e| anyhow::anyhow!("invalid subscription reference address: {}", e))?;

    Ok(ClientBuilder::new(&uri)
        .credentials(Some(onvif::soap::client::Credentials {
            username: conn_data.username.clone(),
            password: conn_data.password.clone(),
        }))
        .auth_type(AuthType::Digest)
        .build())
}

fn has_item(msg: &NotificationMessageHolderType, name: &str, value: &str) -> bool {
    msg.message
        .msg
        .data
        .simple_item
        .iter()
        .any(|si| si.name == name && si.value == value)
}

fn parse_event_type(msg: &PullMessagesResponse) -> Option<CameraEventType> {
    for notification in &msg.notification_message {
        let topic = notification.topic.inner_text.as_str();

        if topic == "tns1:RuleEngine/CellMotionDetector/Motion"
            && has_item(notification, "IsMotion", "true")
        {
            return Some(CameraEventType::Motion);
        }

        // maybe is repeated with upper Motion detection
        // if topic == "tns1:VideoSource/MotionAlarm" && has_item(msg, "State", "true") {
        //     return Some(CameraEventType::Motion);
        // }

        if topic == "tns1:RuleEngine/TamperDetector/Tamper"
            && has_item(notification, "IsTamper", "true")
        {
            return Some(CameraEventType::Tamper);
        }
    }

    None
}

async fn download_picture_from_uri(client: &reqwest::Client, uri: &str) -> anyhow::Result<Vec<u8>> {
    let response = client
        .get(uri)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to download image: {}", e))?;

    if response.status() != 200 {
        bail!("Failed to download image. status:{}", response.status());
    }

    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|_| anyhow::anyhow!("Failed to get bytes of image"))
}

async fn download_picture_from_uri_with_creds(
    client: &reqwest::Client,
    uri: &str,
    credentials: (&str, &str),
) -> anyhow::Result<Vec<u8>> {
    let (username, password) = credentials;
    let response = client
        .get(uri)
        .send_digest_auth((username, password))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to download image with digest auth: {}", e))?;

    if response.status() != 200 {
        bail!("Failed to download image. status:{}", response.status());
    }

    response
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|_| anyhow::anyhow!("Failed to get bytes of image"))
}

async fn download_picture_with_session(
    client: &reqwest::Client,
    uri: &str,
    session: &DigestAuthSession,
) -> anyhow::Result<Vec<u8>> {
    let response = client
        .get(uri)
        .send_digest_auth(session)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to download image with digest auth: {}", e))?;

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
