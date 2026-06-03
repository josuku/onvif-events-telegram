use anyhow::bail;
use app_core::{CameraId, ChatId, SubscriptionId};
use log::error;
use rusqlite::Connection;
use std::sync::Mutex;
// use crate::memory_repository::ChatId;
// use teloxide::types::ChatId;

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

impl Default for DbStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DbStore {
    pub fn new() -> Self {
        Self {
            connection: Mutex::new(Connection::open("./repo_store.db").unwrap()),
        }
    }

    pub fn create_tables(&self) {
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

        query = "
            CREATE TABLE IF NOT EXISTS daily_report_subscriptions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id INTEGER NOT NULL
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
                chat_id: row.get("Chat_id")?,
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
        username: &str,
        password: &str,
        snapshot_uri: &Option<String>,
    ) -> anyhow::Result<CameraId> {
        let connection = self.connection.lock().unwrap();

        if let Some(snapshot_uri) = snapshot_uri {
            if let Err(err) = connection.execute(
                "INSERT INTO cameras (name, uri, address, username, password, snapshot_uri) 
                    values (?1, ?2, ?3, ?4, ?5, ?6)",
                [
                    name,
                    uri,
                    address,
                    username,
                    password,
                    snapshot_uri.as_str(),
                ],
            ) {
                bail!("cannot insert camera: {}", err)
            }
        } else if let Err(err) = connection.execute(
            "INSERT INTO cameras (name, uri, address, username, password) 
                values (?1, ?2, ?3, ?4, ?5)",
            [name, uri, address, username, password],
        ) {
            bail!("cannot insert camera: {}", err)
        }

        Ok(connection.last_insert_rowid())
    }

    pub fn update_uri_from_camera(&self, camera_id: CameraId, uri: &str) {
        let connection = self.connection.lock().unwrap();

        connection
            .execute(
                "UPDATE cameras SET uri = ?1 WHERE id = ?2",
                [uri, &camera_id.to_string()],
            )
            .unwrap();
    }

    pub fn update_name_from_camera(&self, camera_id: CameraId, camera_name: &str) {
        let connection = self.connection.lock().unwrap();

        connection
            .execute(
                "UPDATE cameras SET name = ?1 WHERE id = ?2",
                [camera_name, &camera_id.to_string()],
            )
            .unwrap();
    }

    pub fn update_snapshot_uri_from_camera(&self, camera_id: CameraId, snapshot_uri: &str) {
        let connection = self.connection.lock().unwrap();

        connection
            .execute(
                "UPDATE cameras SET snapshot_uri = ?1 WHERE id = ?2",
                [snapshot_uri, &camera_id.to_string()],
            )
            .unwrap();
    }

    pub fn insert_camera_subscription(
        &self,
        camera_id: CameraId,
        chat_id: ChatId,
    ) -> SubscriptionId {
        let connection = self.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO camera_subscriptions (camera_id, chat_id) 
                values (?1, ?2)",
                [camera_id, chat_id],
            )
            .unwrap();
        connection.last_insert_rowid()
    }

    pub fn remove_camera_subscription(&self, camera_id: CameraId, chat_id: ChatId) {
        let connection = self.connection.lock().unwrap();
        connection
            .execute(
                "DELETE FROM camera_subscriptions WHERE camera_id=(?1) AND chat_id=(?2)",
                [camera_id, chat_id],
            )
            .unwrap();
    }

    pub fn insert_daily_report_subscription(&self, chat_id: ChatId) -> SubscriptionId {
        let connection = self.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO daily_report_subscriptions (chat_id) values (?1)",
                [chat_id],
            )
            .unwrap();
        connection.last_insert_rowid()
    }

    pub fn remove_daily_report_subscription(&self, chat_id: ChatId) {
        let connection = self.connection.lock().unwrap();
        connection
            .execute(
                "DELETE FROM daily_report_subscriptions WHERE chat_id=(?1)",
                [chat_id],
            )
            .unwrap();
    }

    pub fn get_daily_report_subscriptors(&self) -> anyhow::Result<Vec<ChatId>> {
        let connection = self.connection.lock().unwrap();

        let mut chat_ids: Vec<ChatId> = Vec::new();

        let mut stmt = connection.prepare("SELECT chat_id FROM daily_report_subscriptions")?;

        let db_chat_ids = stmt.query_map([], |row| row.get("Chat_id"))?;

        for chat_id in db_chat_ids {
            match chat_id {
                Ok(chat_id) => chat_ids.push(chat_id),
                Err(err) => error!("cannot get chat_id: {}", err),
            }
        }

        Ok(chat_ids)
    }
}
