use anyhow::bail;
use app_core::{
    CameraId, ChatId, SubscriptionId,
    domain::{
        camera::CameraConnectionData,
        config::{BotConfig, DetectorConfig},
        object::{object_classes_to_string, string_to_object_classes},
    },
};
use rusqlite::Connection;
use std::sync::Mutex;
use tracing::{error, info};

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

        query = "
            CREATE TABLE IF NOT EXISTS settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                polling_seconds INTEGER NOT NULL,
                between_seconds INTEGER NOT NULL,
                send_errors BOOLEAN NOT NULL,
                auto_renewal BOOLEAN NOT NULL,
                recording_clip INTEGER NOT NULL,
                detector_enable BOOLEAN NOT NULL,
                detector_min_confidence DECIMAL NOT NULL,
                detector_types TEXT NOT NULL,
                updated_by_chat_id INTEGER NOT NULL
            );
        ";
        connection.execute(query, ()).unwrap();
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
            info!(
                "found camera id:{} name:{} subscriptors:{:?}",
                cam.id, cam.name, cam.subscriptors
            )
        });

        Ok(cameras)
    }

    pub fn insert_camera(
        &self,
        name: &str,
        address: &str,
        conn_data: &CameraConnectionData,
        snapshot_uri: &Option<String>,
    ) -> anyhow::Result<CameraId> {
        let connection = self.connection.lock().unwrap();

        if let Some(snapshot_uri) = snapshot_uri {
            if let Err(err) = connection.execute(
                "INSERT INTO cameras (name, uri, address, username, password, snapshot_uri) 
                    values (?1, ?2, ?3, ?4, ?5, ?6)",
                [
                    name,
                    &conn_data.uri,
                    address,
                    &conn_data.username,
                    &conn_data.password,
                    snapshot_uri.as_str(),
                ],
            ) {
                bail!("cannot insert camera: {}", err)
            }
        } else if let Err(err) = connection.execute(
            "INSERT INTO cameras (name, uri, address, username, password) 
                values (?1, ?2, ?3, ?4, ?5)",
            [
                name,
                &conn_data.uri,
                address,
                &conn_data.username,
                &conn_data.password,
            ],
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

    pub fn update_credentials_from_camera(
        &self,
        camera_id: CameraId,
        username: &str,
        password: &str,
    ) {
        let connection = self.connection.lock().unwrap();
        connection
            .execute(
                "UPDATE cameras SET username = ?1, password = ?2 WHERE id = ?3",
                rusqlite::params![username, password, camera_id],
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

    pub fn delete_camera(&self, camera_id: CameraId) {
        let connection = self.connection.lock().unwrap();

        connection
            .execute(
                "DELETE FROM camera_subscriptions WHERE camera_id = ?1",
                [camera_id],
            )
            .unwrap();

        connection
            .execute("DELETE FROM cameras WHERE id = ?1", [camera_id])
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

    pub fn update_config(&self, chat_id: ChatId, bot_config: &BotConfig) {
        let connection = self.connection.lock().unwrap();
        connection
            .execute(
                "INSERT OR REPLACE INTO settings 
                        (id, polling_seconds, between_seconds, send_errors, auto_renewal, recording_clip, detector_enable, detector_min_confidence, detector_types, updated_by_chat_id)
                      VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9);",
            (bot_config.polling_seconds as i64,
                    bot_config.between_seconds as i64,
                    bot_config.send_errors,
                    bot_config.auto_renewal,
                    bot_config.recording_clip,
                    bot_config.detector.enable,
                    bot_config.detector.min_confidence,
                    object_classes_to_string(&bot_config.detector.types),
                    chat_id)
            )
            .unwrap();
    }

    pub fn get_config(&self) -> anyhow::Result<Option<BotConfig>> {
        let connection = self.connection.lock().unwrap();
        let mut stmt = connection.prepare(
            "SELECT polling_seconds, between_seconds, send_errors, auto_renewal, recording_clip, detector_enable, detector_min_confidence, detector_types FROM settings WHERE id = 1",
        )?;
        let mut rows = stmt.query_map([], |row| {
            let polling_seconds: i64 = row.get(0)?;
            let between_seconds: i64 = row.get(1)?;
            let send_errors: bool = row.get(2)?;
            let auto_renewal: bool = row.get(3)?;
            let recording_clip: i64 = row.get(4)?;
            let detector_enable: bool = row.get(5)?;
            let detector_min_confidence: f32 = row.get(6)?;
            let object_types: String = row.get(7)?;
            Ok(BotConfig {
                polling_seconds: polling_seconds as u64,
                between_seconds: between_seconds as u64,
                send_errors,
                auto_renewal,
                recording_clip: recording_clip as u64,
                detector: DetectorConfig {
                    enable: detector_enable,
                    min_confidence: detector_min_confidence,
                    types: string_to_object_classes(&object_types),
                },
            })
        })?;
        if let Some(config) = rows.next() {
            Ok(Some(config?))
        } else {
            Ok(None)
        }
    }

    pub fn delete_config(&self) {
        let connection = self.connection.lock().unwrap();
        connection
            .execute("DELETE FROM settings WHERE id = 1", [])
            .unwrap();
    }
}
