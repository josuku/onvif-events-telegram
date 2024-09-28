extern crate onvif;

use std::collections::HashSet;

use futures_util::stream::StreamExt;
use log::debug;
use onvif::discovery::{self, Device};
use onvif::soap::client::Client;
use onvif::soap::{self, client::AuthType};
use schema::{self, transport};
use url::Url;

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

// async fn get_capabilities(clients: &OnvifClients) {
//     match schema::devicemgmt::get_capabilities(&clients.devicemgmt, &Default::default()).await {
//         Ok(capabilities) => println!("{:#?}", capabilities),
//         Err(error) => println!("Failed to fetch capabilities: {}", error),
//     }
// }

// async fn get_device_information(clients: &OnvifClients) -> Result<(), transport::Error> {
//     println!(
//         "{:#?}",
//         &schema::devicemgmt::get_device_information(&clients.devicemgmt, &Default::default())
//             .await?
//     );
//     Ok(())
// }

// async fn get_service_capabilities(clients: &OnvifClients) {
//     match schema::event::get_service_capabilities(&clients.devicemgmt, &Default::default()).await {
//         Ok(capability) => println!("devicemgmt: {:#?}", capability),
//         Err(error) => println!("Failed to fetch devicemgmt: {}", error),
//     }

//     if let Some(ref event) = clients.event {
//         match schema::event::get_service_capabilities(event, &Default::default()).await {
//             Ok(capability) => println!("event: {:#?}", capability),
//             Err(error) => println!("Failed to fetch event: {}", error),
//         }
//     }
//     if let Some(ref deviceio) = clients.deviceio {
//         match schema::event::get_service_capabilities(deviceio, &Default::default()).await {
//             Ok(capability) => println!("deviceio: {:#?}", capability),
//             Err(error) => println!("Failed to fetch deviceio: {}", error),
//         }
//     }
//     if let Some(ref media) = clients.media {
//         match schema::event::get_service_capabilities(media, &Default::default()).await {
//             Ok(capability) => println!("media: {:#?}", capability),
//             Err(error) => println!("Failed to fetch media: {}", error),
//         }
//     }
//     if let Some(ref media2) = clients.media2 {
//         match schema::event::get_service_capabilities(media2, &Default::default()).await {
//             Ok(capability) => println!("media2: {:#?}", capability),
//             Err(error) => println!("Failed to fetch media2: {}", error),
//         }
//     }
//     if let Some(ref imaging) = clients.imaging {
//         match schema::event::get_service_capabilities(imaging, &Default::default()).await {
//             Ok(capability) => println!("imaging: {:#?}", capability),
//             Err(error) => println!("Failed to fetch imaging: {}", error),
//         }
//     }
//     if let Some(ref ptz) = clients.ptz {
//         match schema::event::get_service_capabilities(ptz, &Default::default()).await {
//             Ok(capability) => println!("ptz: {:#?}", capability),
//             Err(error) => println!("Failed to fetch ptz: {}", error),
//         }
//     }
//     if let Some(ref analytics) = clients.analytics {
//         match schema::event::get_service_capabilities(analytics, &Default::default()).await {
//             Ok(capability) => println!("analytics: {:#?}", capability),
//             Err(error) => println!("Failed to fetch analytics: {}", error),
//         }
//     }
// }

// async fn get_system_date_and_time(clients: &OnvifClients) {
//     let date =
//         schema::devicemgmt::get_system_date_and_time(&clients.devicemgmt, &Default::default())
//             .await;
//     println!("{:#?}", date);
// }

// async fn get_stream_uris(clients: &OnvifClients) -> Result<(), transport::Error> {
//     let media_client = clients
//         .media
//         .as_ref()
//         .ok_or_else(|| transport::Error::Other("Client media is not available".into()))?;
//     let profiles = schema::media::get_profiles(media_client, &Default::default()).await?;
//     debug!("get_profiles response: {:#?}", &profiles);
//     let requests: Vec<_> = profiles
//         .profiles
//         .iter()
//         .map(|p: &schema::onvif::Profile| schema::media::GetStreamUri {
//             profile_token: schema::onvif::ReferenceToken(p.token.0.clone()),
//             stream_setup: schema::onvif::StreamSetup {
//                 stream: schema::onvif::StreamType::RtpUnicast,
//                 transport: schema::onvif::Transport {
//                     protocol: schema::onvif::TransportProtocol::Rtsp,
//                     tunnel: vec![],
//                 },
//             },
//         })
//         .collect();

//     let responses = futures_util::future::try_join_all(
//         requests
//             .iter()
//             .map(|r| schema::media::get_stream_uri(media_client, r)),
//     )
//     .await?;
//     for (p, resp) in profiles.profiles.iter().zip(responses.iter()) {
//         println!("token={} name={}", &p.token.0, &p.name.0);
//         println!("    {}", &resp.media_uri.uri);
//         if let Some(ref v) = p.video_encoder_configuration {
//             println!(
//                 "    {:?}, {}x{}",
//                 v.encoding, v.resolution.width, v.resolution.height
//             );
//             if let Some(ref r) = v.rate_control {
//                 println!("    {} fps, {} kbps", r.frame_rate_limit, r.bitrate_limit);
//             }
//         }
//         if let Some(ref a) = p.audio_encoder_configuration {
//             println!(
//                 "    audio: {:?}, {} kbps, {} kHz",
//                 a.encoding, a.bitrate, a.sample_rate
//             );
//         }
//     }
//     Ok(())
// }

// pub async fn get_users(uri: &str) -> Result<(), transport::Error> {
//     let base_uri: Url = Url::parse(uri).unwrap();
//     let devicemgmt_uri = base_uri.join("/onvif/device_service").unwrap();
//     let devicemgmt = soap::client::ClientBuilder::new(&devicemgmt_uri)
//         // .credentials(None)
//         // .auth_type(AuthType::Any)
//         .build();

//     let users = schema::devicemgmt::get_users(&devicemgmt, &Default::default()).await?;

//     println!("users: {:?}", users);
//     Ok(())
// }

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

// async fn enable_analytics(clients: &OnvifClients) -> Result<(), transport::Error> {
//     let media_client = clients
//         .media
//         .as_ref()
//         .ok_or_else(|| transport::Error::Other("Client media is not available".into()))?;
//     let mut config =
//         schema::media::get_metadata_configurations(media_client, &Default::default()).await?;
//     if config.configurations.len() != 1 {
//         println!("Expected exactly one analytics config");
//         return Ok(());
//     }
//     let mut c = config.configurations.pop().unwrap();
//     let token_str = c.token.0.clone();
//     println!("{:#?}", &c);
//     if c.analytics != Some(true) || c.events.is_none() {
//         println!(
//             "Enabling analytics in metadata configuration {}",
//             &token_str
//         );
//         c.analytics = Some(true);
//         c.events = Some(schema::onvif::EventSubscription {
//             filter: None,
//             subscription_policy: None,
//         });
//         schema::media::set_metadata_configuration(
//             media_client,
//             &schema::media::SetMetadataConfiguration {
//                 configuration: c,
//                 force_persistence: true,
//             },
//         )
//         .await?;
//     } else {
//         println!(
//             "Analytics already enabled in metadata configuration {}",
//             &token_str
//         );
//     }

//     let profiles = schema::media::get_profiles(media_client, &Default::default()).await?;
//     let requests: Vec<_> = profiles
//         .profiles
//         .iter()
//         .filter_map(
//             |p: &schema::onvif::Profile| match p.metadata_configuration {
//                 Some(_) => None,
//                 None => Some(schema::media::AddMetadataConfiguration {
//                     profile_token: schema::onvif::ReferenceToken(p.token.0.clone()),
//                     configuration_token: schema::onvif::ReferenceToken(token_str.clone()),
//                 }),
//             },
//         )
//         .collect();
//     if !requests.is_empty() {
//         println!(
//             "Enabling metadata on {}/{} configs",
//             requests.len(),
//             profiles.profiles.len()
//         );
//         futures_util::future::try_join_all(
//             requests
//                 .iter()
//                 .map(|r| schema::media::add_metadata_configuration(media_client, r)),
//         )
//         .await?;
//     } else {
//         println!(
//             "Metadata already enabled on {} configs",
//             profiles.profiles.len()
//         );
//     }
//     Ok(())
// }

// async fn get_analytics(clients: &OnvifClients) -> Result<(), transport::Error> {
//     let media_client = clients
//         .media
//         .as_ref()
//         .ok_or_else(|| transport::Error::Other("Client media is not available".into()))?;
//     let config =
//         schema::media::get_video_analytics_configurations(media_client, &Default::default())
//             .await?;

//     println!("{:#?}", &config);
//     let c = match config.configurations.first() {
//         Some(c) => c,
//         None => return Ok(()),
//     };
//     if let Some(ref a) = clients.analytics {
//         let mods = schema::analytics::get_supported_analytics_modules(
//             a,
//             &schema::analytics::GetSupportedAnalyticsModules {
//                 configuration_token: schema::onvif::ReferenceToken(c.token.0.clone()),
//             },
//         )
//         .await?;
//         println!("{:#?}", &mods);
//     }

//     Ok(())
// }

// async fn get_status(clients: &OnvifClients) -> Result<(), transport::Error> {
//     if let Some(ref ptz) = clients.ptz {
//         let media_client = match clients.media.as_ref() {
//             Some(client) => client,
//             None => {
//                 return Err(transport::Error::Other(
//                     "Client media is not available".into(),
//                 ))
//             }
//         };
//         let profile = &schema::media::get_profiles(media_client, &Default::default())
//             .await?
//             .profiles[0];
//         let profile_token = schema::onvif::ReferenceToken(profile.token.0.clone());
//         let status =
//             &schema::ptz::get_status(ptz, &schema::ptz::GetStatus { profile_token }).await?;
//         println!("ptz status: {:#?}", status);
//     }
//     Ok(())
// }
