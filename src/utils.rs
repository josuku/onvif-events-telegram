use crate::onvif::onvif_clients::{
    create_default_user, get_users, DEFAULT_PASSWORD, DEFAULT_USERNAME,
};
use anyhow::bail;
use url::Url;

pub fn make_caption(title: &str, name: &str, time: &chrono::DateTime<chrono::Utc>) -> String {
    let converted: chrono::DateTime<chrono::Local> = chrono::DateTime::from(*time);
    // println!("utc:{} local:{}", time, converted);
    format!(
        r#"
{}
Camera:{}
Time: {}"#,
        title.to_uppercase(),
        name,
        converted
    )
}

pub async fn create_onvif_user_and_fix_snapshot_uri(
    camera_uri: &str,
    orig_snapshot_uri: &str,
) -> anyhow::Result<String> {
    match get_users(camera_uri).await {
        Ok(users) => {
            if !users.contains(&DEFAULT_USERNAME.to_string()) {
                if let Err(err) = create_default_user(camera_uri).await {
                    bail!("cannot create user {}. error:{}", DEFAULT_USERNAME, err);
                }
            }
            Ok(replace_snapshot_uri_credentials(
                orig_snapshot_uri,
                DEFAULT_USERNAME,
                DEFAULT_PASSWORD,
            ))
        }
        Err(_) => {
            bail!("cannot get users of camera:{}", camera_uri)
        }
    }
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
