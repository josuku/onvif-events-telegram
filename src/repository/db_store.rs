use crate::{CameraId, SubscriptionId};
use log::error;
use onvif::soap::client::Credentials;
use rusqlite::Connection;
use std::sync::Mutex;
use teloxide::types::ChatId;

pub struct DbCamera {
    pub id: CameraId,
    pub name: String,
    pub uri: String,
    pub address: String,
    pub username: String,
    pub password: String,
    pub snapshot_uri: Option<String>,
    pub subscriptors: Vec<ChatId>,
}

pub struct DbCameraSubscription {
    pub camera_id: CameraId,
    pub chat_id: ChatId,
}

pub struct DbStore {
    connection: Mutex<Connection>,
}

impl DbStore {
    pub fn new() -> Self {
        Self {
            connection: Mutex::new(Connection::open("repo_store.db").unwrap()),
        }
    }

    pub fn load(&self) {
        let connection: std::sync::MutexGuard<'_, Connection> = self.connection.lock().unwrap();
        let mut query = "
            CREATE TABLE IF NOT EXISTS cameras (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT,
                uri TEXT,
                address TEXT NOT NULL UNIQUE,
                username TEXT,
                password TEXT,
                snapshot_uri TEXT NULL
            );
        ";
        connection.execute(query, ()).unwrap();

        query = "
            CREATE TABLE IF NOT EXISTS camera_subscriptions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                camera_id INTEGER NOT NULL,
                chat_id INTEGER NOT NULL,
                FOREIGN KEY(camera_id) REFERENCES cameras(id)
            );
        ";
        connection.execute(query, ()).unwrap();

        // TODO polling seconds in config table?
    }

    pub fn get_cameras(&self) -> anyhow::Result<Vec<DbCamera>> {
        let connection = self.connection.lock().unwrap();

        let mut stmt = connection.prepare(
            "SELECT id, name, uri, address, username, password, snapshot_uri FROM cameras",
        )?;

        let stored_cameras = stmt.query_map([], |row| {
            Ok(DbCamera {
                id: row.get("id")?,
                name: row.get("name")?,
                uri: row.get("uri")?,
                address: row.get("address")?,
                username: row.get("username")?,
                password: row.get("password")?,
                snapshot_uri: row.get("snapshot_uri")?,
                subscriptors: Vec::new(),
            })
        })?;

        let mut cameras = Vec::new();
        for stored_camera in stored_cameras {
            match stored_camera {
                Ok(camera) => cameras.push(camera),
                Err(err) => error!("cannot get camera: {}", err),
            }
        }

        stmt = connection.prepare("SELECT camera_id, chat_id FROM camera_subscriptions")?;

        let stored_subscriptions = stmt.query_map([], |row| {
            Ok(DbCameraSubscription {
                camera_id: row.get("camera_id")?,
                chat_id: ChatId(row.get("Chat_id")?),
            })
        })?;
        for stored_subscription in stored_subscriptions {
            match stored_subscription {
                Ok(subscription) => {
                    for camera in &mut cameras {
                        if camera.id == subscription.camera_id {
                            camera.subscriptors.push(subscription.chat_id);
                        }
                    }
                }
                Err(err) => error!("cannot get subscription: {}", err),
            }
        }
        cameras.iter().for_each(|cam| {
            println!(
                "found camera id:{} name:{} subscriptors:{:?}",
                cam.id, cam.name, cam.subscriptors
            )
        });

        Ok(cameras)
    }

    pub fn insert_camera(
        &self,
        name: &str,
        uri: &str,
        address: &str,
        credentials: &Credentials,
        snapshot_uri: &Option<String>,
    ) -> CameraId {
        let connection = self.connection.lock().unwrap();

        if let Some(snapshot_uri) = snapshot_uri {
            connection
                .execute(
                    "INSERT INTO cameras (name, uri, address, username, password, snapshot_uri) 
                    values (?1, ?2, ?3, ?4, ?5, ?6)",
                    [
                        name,
                        uri,
                        address,
                        credentials.username.as_str(),
                        credentials.password.as_str(),
                        snapshot_uri.as_str(),
                    ],
                )
                .unwrap();
        } else {
            connection
                .execute(
                    "INSERT INTO cameras (name, uri, address, username, password) 
                    values (?1, ?2, ?3, ?4, ?5)",
                    [
                        name,
                        uri,
                        address,
                        credentials.username.as_str(),
                        credentials.password.as_str(),
                    ],
                )
                .unwrap();
        }

        connection.last_insert_rowid()
    }

    pub fn insert_subscription(&self, camera_id: CameraId, chat_id: ChatId) -> SubscriptionId {
        let connection = self.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO camera_subscriptions (camera_id, chat_id) 
                values (?1, ?2)",
                [camera_id, chat_id.0],
            )
            .unwrap();
        connection.last_insert_rowid()
    }

    pub fn remove_subscription(&self, camera_id: CameraId, chat_id: ChatId) {
        let connection = self.connection.lock().unwrap();
        connection
            .execute(
                "DELETE FROM camera_subscriptions WHERE camera_id=(?1) AND chat_id=(?2)",
                [camera_id, chat_id.0],
            )
            .unwrap();
    }
}
