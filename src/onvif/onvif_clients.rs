extern crate onvif;

use futures_util::stream::StreamExt;
use log::{debug, error};
use onvif::discovery::{self, Device};
use onvif::soap::client::Client;
use onvif::soap::{self, client::AuthType};
use schema::devicemgmt::CreateUsers;
use schema::{self, transport};
use std::collections::HashSet;
use url::Url;

pub const DEFAULT_USERNAME: &str = "oet1";
pub const DEFAULT_PASSWORD: &str = "oet12345";

pub async fn camera_discovery() -> Vec<Device> {
    discovery::DiscoveryBuilder::default()
        .run()
        .await
        .unwrap()
        .collect::<Vec<Device>>()
        .await
}

#[derive(Clone)]
pub struct OnvifClients {
    pub devicemgmt: soap::client::Client,
    pub event: Option<soap::client::Client>,
    pub deviceio: Option<soap::client::Client>,
    pub media: Option<soap::client::Client>,
    pub media2: Option<soap::client::Client>,
    pub imaging: Option<soap::client::Client>,
    pub ptz: Option<soap::client::Client>,
    pub analytics: Option<soap::client::Client>,
}

impl OnvifClients {
    pub async fn new(
        uri: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<Self, String> {
        let creds = match (username.as_ref(), password.as_ref()) {
            (Some(username), Some(password)) => Some(soap::client::Credentials {
                username: username.to_string(),
                password: password.to_string(),
            }),
            (None, None) => None,
            _ => panic!("username and password must be specified together"),
        };
        println!("new OnvifClients. uri:{}", uri);
        let base_uri: Url = Url::parse(uri).unwrap();
        let devicemgmt_uri = base_uri.join("/onvif/device_service").unwrap();
        let auth_type = AuthType::Any;
        let mut out = Self {
            devicemgmt: soap::client::ClientBuilder::new(&devicemgmt_uri)
                .credentials(creds.clone())
                .auth_type(auth_type.clone())
                .build(),
            imaging: None,
            ptz: None,
            event: None,
            deviceio: None,
            media: None,
            media2: None,
            analytics: None,
        };

        // let time_gap = if args.fix_time {
        //     let device_time =
        //         schema::devicemgmt::get_system_date_and_time(&out.devicemgmt, &Default::default())
        //             .await?
        //             .system_date_and_time;

        //     if let Some(utc_time) = &device_time.utc_date_time {
        //         let pc_time = Utc::now();
        //         let date = &utc_time.date;
        //         let t = &utc_time.time;
        //         let device_time =
        //             NaiveDate::from_ymd_opt(date.year, date.month as _, date.day as _)
        //                 .unwrap()
        //                 .and_hms_opt(t.hour as _, t.minute as _, t.second as _)
        //                 .unwrap()
        //                 .and_utc();

        //         let diff = device_time - pc_time;
        //         if diff.num_seconds().abs() > 60 {
        //             out.devicemgmt.set_fix_time_gap(Some(diff));
        //         }
        //         Some(diff)
        //     } else {
        //         warn!("GetSystemDateAndTimeResponse doesn't have utc_data_time value!");
        //         None
        //     }
        // } else {
        //     None
        // };
        let time_gap = None;

        let services =
            schema::devicemgmt::get_services(&out.devicemgmt, &Default::default()).await?;
        for service in &services.service {
            let service_url = Url::parse(&service.x_addr).map_err(|e| e.to_string())?;
            if !service_url.as_str().starts_with(base_uri.as_str()) {
                return Err(format!(
                    "Service URI {} is not within base URI {}",
                    service_url, base_uri
                ));
            }
            let svc = Some(
                soap::client::ClientBuilder::new(&service_url)
                    .credentials(creds.clone())
                    .auth_type(auth_type.clone())
                    .fix_time_gap(time_gap)
                    .build(),
            );
            match service.namespace.as_str() {
                "http://www.onvif.org/ver10/device/wsdl" => {
                    if service_url != devicemgmt_uri {
                        return Err(format!(
                            "advertised device mgmt uri {} not expected {}",
                            service_url, devicemgmt_uri
                        ));
                    }
                }
                "http://www.onvif.org/ver10/events/wsdl" => out.event = svc,
                "http://www.onvif.org/ver10/deviceIO/wsdl" => out.deviceio = svc,
                "http://www.onvif.org/ver10/media/wsdl" => out.media = svc,
                "http://www.onvif.org/ver20/media/wsdl" => out.media2 = svc,
                "http://www.onvif.org/ver20/imaging/wsdl" => out.imaging = svc,
                "http://www.onvif.org/ver20/ptz/wsdl" => out.ptz = svc,
                "http://www.onvif.org/ver20/analytics/wsdl" => out.analytics = svc,
                _ => debug!("unknown service: {:?}", service),
            }
        }
        Ok(out)
    }
}

fn get_devicemgmt(uri: &str) -> Client {
    let base_uri: Url = Url::parse(uri).unwrap();
    let devicemgmt_uri = base_uri.join("/onvif/device_service").unwrap();
    soap::client::ClientBuilder::new(&devicemgmt_uri).build()
}

pub async fn get_users(camera_uri: &str) -> Result<Vec<String>, transport::Error> {
    let devicemgmt = get_devicemgmt(camera_uri);
    let users = schema::devicemgmt::get_users(&devicemgmt, &Default::default()).await?;
    Ok(users
        .user
        .iter()
        .map(|user| user.username.clone())
        .collect())
}

pub async fn create_default_user(camera_uri: &str) -> Result<(), transport::Error> {
    let devicemgmt = get_devicemgmt(camera_uri);
    let new_user = onvif_xsd::User {
        username: DEFAULT_USERNAME.to_string(),
        password: Some(DEFAULT_PASSWORD.to_string()),
        user_level: onvif_xsd::UserLevel::Administrator,
        extension: None,
    };

    let create_users = CreateUsers {
        user: vec![new_user],
    };

    if let Err(err) = schema::devicemgmt::create_users(&devicemgmt, &create_users).await {
        error!("cannot create user: {}", err);
    };

    // let mut delete_users = Vec::new();
    // delete_users.push("oet1".to_string());
    // let delete_users = DeleteUsers {
    //     username: delete_users
    // };
    // if let Err(err) =  schema::devicemgmt::delete_users(&devicemgmt, &delete_users).await {
    //     error!("cannot delete user: {}", err);
    // };

    let users: schema::devicemgmt::GetUsersResponse =
        schema::devicemgmt::get_users(&devicemgmt, &Default::default()).await?;
    println!("users after creation: {:?}", users);

    Ok(())
}

pub async fn get_snapshot_uris(media_client: &Client) -> Result<Vec<String>, transport::Error> {
    let mut uris = Vec::new();
    let profiles = schema::media::get_profiles(media_client, &Default::default()).await?;
    debug!("get_profiles response: {:#?}", &profiles);
    let requests: Vec<_> = profiles
        .profiles
        .iter()
        .map(|p: &schema::onvif::Profile| schema::media::GetSnapshotUri {
            profile_token: schema::onvif::ReferenceToken(p.token.0.clone()),
        })
        .collect();

    let responses = futures_util::future::try_join_all(
        requests
            .iter()
            .map(|r| schema::media::get_snapshot_uri(media_client, r)),
    )
    .await?;
    for (p, resp) in profiles.profiles.iter().zip(responses.iter()) {
        println!("token={} name={}", &p.token.0, &p.name.0);
        println!("    snapshot_uri={}", &resp.media_uri.uri);
        uris.push(resp.media_uri.uri.clone());
    }

    Ok(uris
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>())
}

// async fn get_hostname(clients: &OnvifClients) -> Result<(), transport::Error> {
//     let resp = schema::devicemgmt::get_hostname(&clients.devicemgmt, &Default::default()).await?;
//     debug!("get_hostname response: {:#?}", &resp);
//     println!(
//         "{}",
//         resp.hostname_information
//             .name
//             .as_deref()
//             .unwrap_or("(unset)")
//     );
//     Ok(())
// }

// async fn set_hostname(clients: &OnvifClients, hostname: String) -> Result<(), transport::Error> {
//     schema::devicemgmt::set_hostname(
//         &clients.devicemgmt,
//         &schema::devicemgmt::SetHostname { name: hostname },
//     )
//     .await?;
//     Ok(())
// }
