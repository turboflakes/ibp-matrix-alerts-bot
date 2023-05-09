// The MIT License (MIT)
// Copyright (c) 2023 IBP.network
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

#![allow(dead_code)]
use crate::abot::{MemberId, MuteTime, ReportType, Severity};
use crate::cache::{create_or_await_pool, get_conn, CacheKey, RedisPool};
use crate::config::CONFIG;
use crate::errors::{CacheError, MatrixError};
use actix_web::web;
use async_recursion::async_recursion;
use base64::encode;
use log::{debug, info, warn};
use redis::aio::Connection;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, collections::HashSet};
use std::{fs, fs::File, result::Result, thread, time};
use url::form_urlencoded::byte_serialize;

const MATRIX_URL: &str = "https://matrix.org/_matrix/client/r0";
const MATRIX_MEDIA_URL: &str = "https://matrix.org/_matrix/media/r0";
const MATRIX_BOT_NAME: &str = "IBP ALERTS";
const MATRIX_NEXT_TOKEN_FILENAME: &str = ".next_token";

type AccessToken = String;
type SyncToken = String;
type RoomID = String;
type EventID = String;
type URI = String;
pub type UserID = String;

#[derive(Debug, Deserialize, Clone, PartialEq)]
enum Commands {
    Help,
    Subscribe(ReportType, UserID),
    SubscribeAll(ReportType, UserID),
    Unsubscribe(ReportType, UserID),
    UnsubscribeAll(ReportType, UserID),
    NotSupported,
}

#[derive(Deserialize, Debug, Default)]
struct Room {
    #[serde(default)]
    room_id: RoomID,
    #[serde(default)]
    servers: Vec<String>,
    #[serde(default)]
    room_alias: String,
    #[serde(default)]
    room_alias_name: String,
}

impl Room {
    fn new_private(user_id: &str) -> Room {
        let config = CONFIG.clone();
        let room_alias_name = define_private_room_alias_name(
            env!("CARGO_PKG_NAME"),
            &user_id,
            &config.matrix_bot_user,
        );
        let v: Vec<&str> = config.matrix_bot_user.split(":").collect();
        Room {
            room_alias_name: room_alias_name.to_string(),
            room_alias: format!("#{}:{}", room_alias_name.to_string(), v.last().unwrap()),
            ..Default::default()
        }
    }
}

impl std::fmt::Display for Room {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.room_alias)
    }
}

fn define_private_room_alias_name(
    pkg_name: &str,
    matrix_user: &str,
    matrix_bot_user: &str,
) -> String {
    encode(format!("{}/{}/{}", pkg_name, matrix_user, matrix_bot_user).as_bytes())
}

#[derive(Debug, Serialize, Deserialize)]
struct LoginRequest {
    r#type: String,
    user: String,
    password: String,
}

#[derive(Deserialize, Debug)]
struct LoginResponse {
    user_id: UserID,
    access_token: AccessToken,
    home_server: String,
    device_id: String,
    // "well_known": {
    //   "m.homeserver": {
    //       "base_url": "https://matrix-client.matrix.org/"
    //   }
    // }
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateRoomRequest {
    name: String,
    room_alias_name: String,
    topic: String,
    preset: String,
    invite: Vec<String>,
    is_direct: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct SendRoomMessageRequest {
    msgtype: String,
    body: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    format: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    formatted_body: String,
    #[serde(skip_serializing_if = "FileInfo::is_empty")]
    info: FileInfo,
    #[serde(skip_serializing_if = "String::is_empty")]
    url: String,
}

impl SendRoomMessageRequest {
    pub fn with_message(message: &str, formatted_message: Option<&str>) -> Self {
        if let Some(formatted_msg) = formatted_message {
            Self {
                msgtype: "m.text".to_string(),
                body: message.to_string(),
                format: "org.matrix.custom.html".to_string(),
                formatted_body: formatted_msg.to_string(),
                ..Default::default()
            }
        } else {
            Self {
                msgtype: "m.text".to_string(),
                body: message.to_string(),
                ..Default::default()
            }
        }
    }

    pub fn with_attachment(filename: &str, url: &str, file_info: Option<FileInfo>) -> Self {
        if let Some(info) = file_info {
            Self {
                msgtype: "m.file".to_string(),
                body: filename.to_string(),
                url: url.to_string(),
                info: FileInfo {
                    mimetype: info.mimetype,
                    size: info.size,
                },
                ..Default::default()
            }
        } else {
            Self {
                msgtype: "m.file".to_string(),
                body: filename.to_string(),
                url: url.to_string(),
                ..Default::default()
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct FileInfo {
    mimetype: String,
    size: u64,
}

impl FileInfo {
    pub fn with_size(size: u64) -> Self {
        Self {
            mimetype: "text/plain".to_string(),
            size,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.mimetype.is_empty() && self.size == 0
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RoomEventFilter {
    types: Vec<String>,
    rooms: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct RoomEventsResponse {
    chunk: Vec<ClientEvent>,
    #[serde(default)]
    start: SyncToken,
    #[serde(default)]
    end: SyncToken,
}

#[derive(Deserialize, Debug)]
struct ClientEvent {
    content: EventContent,
    origin_server_ts: u64,
    room_id: String,
    sender: String,
    r#type: String,
    // unsigned
    event_id: String,
    user_id: String,
    #[serde(skip)]
    age: u32,
}

#[derive(Deserialize, Debug)]
struct EventContent {
    #[serde(default)]
    body: String,
    #[serde(default)]
    msgtype: String,
    #[serde(default)]
    displayname: String,
    #[serde(default)]
    membership: String,
}

#[derive(Deserialize, Debug)]
struct SendRoomMessageResponse {
    event_id: EventID,
}

#[derive(Deserialize, Debug)]
struct JoinedRoomsResponse {
    joined_rooms: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct SyncResponse {
    next_batch: String,
}

#[derive(Deserialize, Debug)]
struct UploadResponse {
    content_uri: String,
}

#[derive(Deserialize, Debug)]
struct ErrorResponse {
    errcode: String,
    error: String,
}

#[derive(Clone)]
pub struct Matrix {
    pub client: reqwest::Client,
    access_token: Option<String>,
    public_room_id: String,
    callout_public_room_ids: Vec<String>,
    disabled: bool,
    cache: RedisPool,
}

impl Default for Matrix {
    fn default() -> Matrix {
        Matrix {
            client: reqwest::Client::new(),
            access_token: None,
            public_room_id: String::from(""),
            callout_public_room_ids: Vec::new(),
            disabled: false,
            cache: create_or_await_pool(CONFIG.clone()),
        }
    }
}

impl Matrix {
    pub fn new() -> Matrix {
        let config = CONFIG.clone();
        Matrix {
            disabled: config.matrix_disabled,
            ..Default::default()
        }
    }

    fn public_room_alias(&self) -> String {
        let config = CONFIG.clone();
        format!("#{}", config.matrix_public_room)
    }

    async fn login(&mut self) -> Result<(), MatrixError> {
        if self.disabled {
            return Ok(());
        }
        let config = CONFIG.clone();
        if let None = config.matrix_bot_user.find(":") {
            return Err(MatrixError::Other(format!("matrix bot user '{}' does not specify the matrix server e.g. '@your-own-bot-account:matrix.org'", config.matrix_bot_user)));
        }
        let client = self.client.clone();
        let req = LoginRequest {
            r#type: "m.login.password".to_string(),
            user: config.matrix_bot_user.to_string(),
            password: config.matrix_bot_password.to_string(),
        };

        let res = client
            .post(format!("{}/login", MATRIX_URL))
            .json(&req)
            .send()
            .await?;

        debug!("response {:?}", res);
        match res.status() {
            reqwest::StatusCode::OK => {
                let response = res.json::<LoginResponse>().await?;
                self.access_token = Some(response.access_token);
                info!(
                    "The '{} Bot' user {} has been authenticated at {}",
                    MATRIX_BOT_NAME, response.user_id, response.home_server
                );
                Ok(())
            }
            _ => {
                let response = res.json::<ErrorResponse>().await?;
                Err(MatrixError::Other(response.error))
            }
        }
    }

    #[allow(dead_code)]
    pub async fn logout(&mut self) -> Result<(), MatrixError> {
        if self.disabled {
            return Ok(());
        }
        match &self.access_token {
            Some(access_token) => {
                let client = self.client.clone();
                let res = client
                    .post(format!(
                        "{}/logout?access_token={}",
                        MATRIX_URL, access_token
                    ))
                    .send()
                    .await?;
                debug!("response {:?}", res);
                match res.status() {
                    reqwest::StatusCode::OK => {
                        self.access_token = None;
                        Ok(())
                    }
                    _ => {
                        let response = res.json::<ErrorResponse>().await?;
                        Err(MatrixError::Other(response.error))
                    }
                }
            }
            None => Err(MatrixError::Other("access_token not defined".to_string())),
        }
    }

    pub async fn authenticate(&mut self) -> Result<(), MatrixError> {
        self.silent_authentication().await?;
        info!(
            "Messages will be sent to public room {}",
            self.public_room_alias()
        );
        Ok(())
    }

    // Login user and join public room
    async fn silent_authentication(&mut self) -> Result<(), MatrixError> {
        if self.disabled {
            return Ok(());
        }
        let config = CONFIG.clone();
        // Login
        self.login().await?;
        // Verify if user did not disabled public room in config
        if !config.matrix_public_room_disabled {
            // Join public room if not a member
            match self
                .get_room_id_by_room_alias(&self.public_room_alias())
                .await?
            {
                Some(public_room_id) => {
                    // Join room if not already a member
                    let joined_rooms = self.get_joined_rooms().await?;
                    debug!("joined_rooms {:?}", joined_rooms);
                    if !joined_rooms.contains(&public_room_id) {
                        self.join_room(&public_room_id).await?;
                    }
                    self.public_room_id = public_room_id;
                }
                None => {
                    return Err(MatrixError::Other(format!(
                        "Public room {} not found.",
                        self.public_room_alias()
                    )))
                }
            }
        }
        Ok(())
    }

    pub async fn lazy_load_and_process_commands(&self) -> Result<(), MatrixError> {
        // get members for joined members for the public room
        let members = self.get_members_from_room(&self.public_room_id).await?;
        info!(
            "Loading {} members from public room {}.",
            members.len(),
            self.public_room_alias()
        );
        // verify that all members have their private rooms created
        let mut private_rooms: HashSet<RoomID> = HashSet::new();
        for member in members.iter() {
            if let Some(private_room) = self.get_or_create_private_room(member).await? {
                private_rooms.insert(private_room.room_id.to_string());
                info!("Private room {} ready.", private_room);
            }
        }

        while let Some(sync_token) = self.get_next_or_sync().await? {
            // TODO: Remove members that eventually leave public room without the need of restarting the service

            // ### Look for new members that join public room ###
            if let Some(new_members) = self
                .get_members_from_room_and_token(&self.public_room_id)
                .await?
            {
                for member in new_members.iter() {
                    if let Some(private_room) = self.get_or_create_private_room(member).await? {
                        private_rooms.insert(private_room.room_id.to_string());
                        info!(
                            "Private room {} for new member {} ready.",
                            private_room, member
                        );
                    }
                }
            }

            // Read commands from private rooms
            for private_room_id in private_rooms.iter() {
                if let Some(commands) = self.get_commands_from_room(&private_room_id, None).await? {
                    self.process_commands_into_room(commands, &private_room_id)
                        .await?;
                }
            }

            // Read commands from public room
            if let Some(commands) = self
                .get_commands_from_room(&self.public_room_id, Some(sync_token.clone()))
                .await?
            {
                self.process_commands_into_room(commands, &self.public_room_id)
                    .await?;
            }
            thread::sleep(time::Duration::from_secs(6));
        }
        Ok(())
    }

    async fn subscribe_alerts(
        &self,
        who: &str,
        member_id: &str,
        severity: Severity,
        mute_time: MuteTime,
    ) -> Result<(), MatrixError> {
        let mut conn = get_conn(&self.cache).await?;
        let mut data: BTreeMap<String, String> = BTreeMap::new();
        data.insert(String::from("mute"), mute_time.to_string());

        redis::cmd("HSET")
            .arg(CacheKey::SubscriberConfig(
                who.to_string(),
                member_id.to_string(),
                severity.clone(),
            ))
            .arg(data)
            .query_async::<Connection, bool>(&mut conn)
            .await
            .map_err(CacheError::RedisCMDError)?;

        redis::cmd("SADD")
            .arg(CacheKey::Subscribers(
                member_id.to_string(),
                severity.clone(),
            ))
            .arg(who.to_string())
            .query_async::<Connection, bool>(&mut conn)
            .await
            .map_err(CacheError::RedisCMDError)?;

        Ok(())
    }

    async fn unsubscribe_alerts(
        &self,
        who: &str,
        member_id: &str,
        severity: Severity,
    ) -> Result<(), MatrixError> {
        let mut conn = get_conn(&self.cache).await?;

        redis::cmd("SREM")
            .arg(CacheKey::Subscribers(member_id.to_string(), severity))
            .arg(who.to_string())
            .query_async::<Connection, bool>(&mut conn)
            .await
            .map_err(CacheError::RedisCMDError)?;

        Ok(())
    }

    async fn process_commands_into_room(
        &self,
        commands: Vec<Commands>,
        room_id: &str,
    ) -> Result<(), MatrixError> {
        let config = CONFIG.clone();
        for cmd in commands.iter() {
            match cmd {
                Commands::Help => self.reply_help(&room_id).await?,
                Commands::Subscribe(report, who) => match report {
                    ReportType::Alerts(member_optional, severity_optional, mute_time_optional) => {
                        if let Some(member) = member_optional {
                            // cache mute time defined by user otherwise set default
                            let mute_time = if let Some(mt) = mute_time_optional {
                                mt.clone()
                            } else {
                                config.mute_time
                            };

                            // first validate if it's a valid member
                            let mut conn = get_conn(&self.cache).await?;
                            let is_member = redis::cmd("SISMEMBER")
                                .arg(CacheKey::Members)
                                .arg(member.to_string())
                                .query_async::<Connection, bool>(&mut conn)
                                .await
                                .map_err(CacheError::RedisCMDError)?;

                            if is_member {
                                if let Some(severity) = severity_optional {
                                    self.subscribe_alerts(who, member, severity.clone(), mute_time)
                                        .await?;
                                } else {
                                    self.subscribe_alerts(who, member, Severity::High, mute_time)
                                        .await?;
                                    self.subscribe_alerts(who, member, Severity::Medium, mute_time)
                                        .await?;
                                    self.subscribe_alerts(who, member, Severity::Low, mute_time)
                                        .await?;
                                }

                                let message = format!("📥 Subscription -> {} ", report.name());
                                self.send_private_message(who, &message, Some(&message))
                                    .await?;
                            } else {
                                let message = format!(
                                    "❓ No Member with ID <b>{}</b> defined",
                                    member.to_string()
                                );
                                self.send_private_message(who, &message, Some(&message))
                                    .await?;
                            }
                        }
                    }
                },
                Commands::SubscribeAll(report, who) => match report {
                    ReportType::Alerts(_, _, mute_time_optional) => {
                        let mut conn = get_conn(&self.cache).await?;

                        // cache mute time defined by user otherwise set default
                        let mute_time = if let Some(mt) = mute_time_optional {
                            mt.clone()
                        } else {
                            config.mute_time
                        };

                        // get all defined members
                        let member_ids = redis::cmd("SMEMBERS")
                            .arg(CacheKey::Members)
                            .query_async::<Connection, Vec<MemberId>>(&mut conn)
                            .await
                            .map_err(CacheError::RedisCMDError)?;

                        // subscribe every member for all type of severities
                        for member_id in member_ids {
                            self.subscribe_alerts(who, &member_id, Severity::High, mute_time)
                                .await?;
                            self.subscribe_alerts(who, &member_id, Severity::Medium, mute_time)
                                .await?;
                            self.subscribe_alerts(who, &member_id, Severity::Low, mute_time)
                                .await?;
                        }
                        let message = format!("📥 Subscription -> {}", report.name());
                        self.send_private_message(who, &message, Some(&message))
                            .await?;
                    }
                },
                Commands::Unsubscribe(report, who) => match report {
                    ReportType::Alerts(member_optional, severity_optional, _) => {
                        if let Some(member) = member_optional {
                            if let Some(severity) = severity_optional {
                                let mut conn = get_conn(&self.cache).await?;

                                let is_member = redis::cmd("SISMEMBER")
                                    .arg(CacheKey::Subscribers(
                                        member.to_string(),
                                        severity.clone(),
                                    ))
                                    .arg(who.to_string())
                                    .query_async::<Connection, bool>(&mut conn)
                                    .await
                                    .map_err(CacheError::RedisCMDError)?;

                                if is_member {
                                    self.unsubscribe_alerts(who, member, severity.clone())
                                        .await?;

                                    let message = format!(
                                        "🗑️ Subscription removed - <i>{}</i>",
                                        report.name()
                                    );
                                    self.send_private_message(who, &message, Some(&message))
                                        .await?;
                                } else {
                                    let message =
                                        format!("❌ No Subscription - <i>{}</i>", report.name());
                                    self.send_private_message(who, &message, Some(&message))
                                        .await?;
                                }
                            } else {
                                self.unsubscribe_alerts(who, member, Severity::High).await?;
                                self.unsubscribe_alerts(who, member, Severity::Medium)
                                    .await?;
                                self.unsubscribe_alerts(who, member, Severity::Low).await?;

                                let message =
                                    format!("🗑️ Subscription removed - <i>{}</i>", report.name());
                                self.send_private_message(who, &message, Some(&message))
                                    .await?;
                            }
                        }
                    }
                },
                Commands::UnsubscribeAll(report, who) => match report {
                    ReportType::Alerts(_, _, _) => {
                        let mut conn = get_conn(&self.cache).await?;

                        // get all defined members
                        let member_ids = redis::cmd("SMEMBERS")
                            .arg(CacheKey::Members)
                            .query_async::<Connection, Vec<MemberId>>(&mut conn)
                            .await
                            .map_err(CacheError::RedisCMDError)?;

                        // subscribe every member for all type of severities
                        for member_id in member_ids {
                            self.unsubscribe_alerts(who, &member_id, Severity::High)
                                .await?;
                            self.unsubscribe_alerts(who, &member_id, Severity::Medium)
                                .await?;
                            self.unsubscribe_alerts(who, &member_id, Severity::Low)
                                .await?;
                        }
                        let message = format!("🗑️ Subscription removed - <i>{}</i>", report.name());
                        self.send_private_message(who, &message, Some(&message))
                            .await?;
                    }
                },
                _ => (),
            }
        }
        Ok(())
    }

    async fn get_room_id_by_room_alias(
        &self,
        room_alias: &str,
    ) -> Result<Option<RoomID>, MatrixError> {
        let client = self.client.clone();
        let room_alias_encoded: String = byte_serialize(room_alias.as_bytes()).collect();
        let res = client
            .get(format!(
                "{}/directory/room/{}",
                MATRIX_URL, room_alias_encoded
            ))
            .send()
            .await?;
        debug!("response {:?}", res);
        match res.status() {
            reqwest::StatusCode::OK => {
                let room = res.json::<Room>().await?;
                debug!("{} * Matrix room alias", room_alias);
                Ok(Some(room.room_id))
            }
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            _ => {
                let response = res.json::<ErrorResponse>().await?;
                Err(MatrixError::Other(response.error))
            }
        }
    }

    async fn create_private_room(&self, user_id: &str) -> Result<Option<Room>, MatrixError> {
        match &self.access_token {
            Some(access_token) => {
                let client = self.client.clone();
                let room: Room = Room::new_private(user_id);
                let req = CreateRoomRequest {
                    name: format!("{} Bot (Private)", MATRIX_BOT_NAME),
                    room_alias_name: room.room_alias_name.to_string(),
                    topic: format!("{} Bot", MATRIX_BOT_NAME),
                    preset: "trusted_private_chat".to_string(),
                    invite: vec![user_id.to_string()],
                    is_direct: true,
                };
                let res = client
                    .post(format!(
                        "{}/createRoom?access_token={}",
                        MATRIX_URL, access_token
                    ))
                    .json(&req)
                    .send()
                    .await?;

                debug!("response {:?}", res);
                match res.status() {
                    reqwest::StatusCode::OK => {
                        let mut r = res.json::<Room>().await?;
                        r.room_alias = room.room_alias;
                        r.room_alias_name = room.room_alias_name;
                        info!("{} * Matrix private room alias created", r.room_alias);
                        Ok(Some(r))
                    }
                    _ => {
                        let response = res.json::<ErrorResponse>().await?;
                        Err(MatrixError::Other(response.error))
                    }
                }
            }
            None => Err(MatrixError::Other("access_token not defined".to_string())),
        }
    }

    async fn get_or_create_private_room(&self, user_id: &str) -> Result<Option<Room>, MatrixError> {
        match &self.access_token {
            Some(_) => {
                let mut room: Room = Room::new_private(user_id);
                match self.get_room_id_by_room_alias(&room.room_alias).await? {
                    Some(room_id) => {
                        room.room_id = room_id;
                        Ok(Some(room))
                    }
                    None => match self.create_private_room(user_id).await? {
                        Some(room) => {
                            self.reply_help(&room.room_id).await?;
                            Ok(Some(room))
                        }
                        None => Ok(None),
                    },
                }
            }
            None => Err(MatrixError::Other("access_token not defined".to_string())),
        }
    }

    async fn get_joined_rooms(&self) -> Result<Vec<String>, MatrixError> {
        match &self.access_token {
            Some(access_token) => {
                let client = self.client.clone();
                let res = client
                    .get(format!(
                        "{}/joined_rooms?access_token={}",
                        MATRIX_URL, access_token
                    ))
                    .send()
                    .await?;
                debug!("response {:?}", res);
                match res.status() {
                    reqwest::StatusCode::OK => {
                        let response = res.json::<JoinedRoomsResponse>().await?;
                        Ok(response.joined_rooms)
                    }
                    _ => {
                        let response = res.json::<ErrorResponse>().await?;
                        Err(MatrixError::Other(response.error))
                    }
                }
            }
            None => Err(MatrixError::Other("access_token not defined".to_string())),
        }
    }

    // Upload file
    // https://matrix.org/docs/spec/client_server/r0.6.0#m-file
    pub fn upload_file(&self, filename: &str) -> Result<Option<URI>, MatrixError> {
        match &self.access_token {
            Some(access_token) => {
                let file = File::open(filename)?;
                let client = reqwest::blocking::Client::new();
                let res = client
                    .post(format!(
                        "{}/upload?access_token={}",
                        MATRIX_MEDIA_URL, access_token
                    ))
                    .body(file)
                    .send()?;
                match res.status() {
                    reqwest::StatusCode::OK => {
                        let response = res.json::<UploadResponse>()?;
                        Ok(Some(response.content_uri))
                    }
                    _ => {
                        let response = res.json::<ErrorResponse>()?;
                        Err(MatrixError::Other(response.error))
                    }
                }
            }
            None => Err(MatrixError::Other("access_token not defined".to_string())),
        }
    }

    // Sync
    // https://spec.matrix.org/v1.2/client-server-api/#syncing
    async fn get_next_or_sync(&self) -> Result<Option<SyncToken>, MatrixError> {
        let config = CONFIG.clone();
        let next_token_filename = format!(
            "{}{}.{}",
            config.data_path, MATRIX_NEXT_TOKEN_FILENAME, self.public_room_id
        );
        // Try to read first cached token from file
        match fs::read_to_string(&next_token_filename) {
            Ok(token) => Ok(Some(token)),
            _ => {
                match &self.access_token {
                    Some(access_token) => {
                        let client = self.client.clone();
                        let res = client
                            .get(format!("{}/sync?access_token={}", MATRIX_URL, access_token))
                            .send()
                            .await?;
                        match res.status() {
                            reqwest::StatusCode::OK => {
                                let response = res.json::<SyncResponse>().await?;
                                // Persist token to file in case we need to restore commands from previously attempt
                                fs::write(&next_token_filename, &response.next_batch)?;
                                Ok(Some(response.next_batch))
                            }
                            _ => {
                                let response = res.json::<ErrorResponse>().await?;
                                Err(MatrixError::Other(response.error))
                            }
                        }
                    }
                    None => Err(MatrixError::Other("access_token not defined".to_string())),
                }
            }
        }
    }

    // Getting events for a room
    // https://spec.matrix.org/v1.2/client-server-api/#get_matrixclientv3roomsroomidmessages
    async fn get_commands_from_room(
        &self,
        room_id: &str,
        from_token: Option<String>,
    ) -> Result<Option<Vec<Commands>>, MatrixError> {
        match &self.access_token {
            Some(access_token) => {
                let config = CONFIG.clone();
                let next_token_filename = format!(
                    "{}{}.{}",
                    config.data_path, MATRIX_NEXT_TOKEN_FILENAME, room_id
                );

                // If token is None try to read from cached file
                let from_token = match from_token {
                    Some(token) => Some(token),
                    None => match fs::read_to_string(&next_token_filename) {
                        Ok(token) => Some(token),
                        _ => None,
                    },
                };

                //
                let client = self.client.clone();
                let room_id_encoded: String = byte_serialize(room_id.as_bytes()).collect();
                let filter = RoomEventFilter {
                    types: vec!["m.room.message".to_string()],
                    rooms: vec![room_id.to_string()],
                };
                let filter_str = serde_json::to_string(&filter)?;
                let filter_encoded: String = byte_serialize(filter_str.as_bytes()).collect();
                let url = if let Some(token) = from_token {
                    format!(
                        "{}/rooms/{}/messages?access_token={}&from={}&filter={}",
                        MATRIX_URL, room_id_encoded, access_token, token, filter_encoded
                    )
                } else {
                    format!(
                        "{}/rooms/{}/messages?access_token={}&filter={}",
                        MATRIX_URL, room_id_encoded, access_token, filter_encoded
                    )
                };
                let res = client.get(url).send().await?;
                match res.status() {
                    reqwest::StatusCode::OK => {
                        let events = res.json::<RoomEventsResponse>().await?;
                        let mut commands: Vec<Commands> = Vec::new();
                        // Parse message to commands
                        for message in events.chunk.iter() {
                            if message.content.msgtype == "m.text" {
                                let body = message.content.body.trim();
                                match body.split_once(' ') {
                                    None => {
                                        if body == "!help" {
                                            commands.push(Commands::Help);
                                        }
                                    }
                                    Some((cmd, other_params)) => match cmd {
                                        "!subscribe" => match other_params.split_once(' ') {
                                            None => match other_params {
                                                "alerts" => {
                                                    // !subscribe alerts
                                                    commands.push(Commands::SubscribeAll(
                                                        ReportType::Alerts(None, None, None),
                                                        message.sender.to_string(),
                                                    ))
                                                }
                                                _ => commands.push(Commands::NotSupported),
                                            },
                                            Some((report_type, other_params)) => {
                                                match report_type {
                                                    "alerts" => {
                                                        match extract_mute_time(other_params) {
                                                            Some(mute_time) => {
                                                                // !subscribe alerts [10]
                                                                commands.push(
                                                                    Commands::SubscribeAll(
                                                                        ReportType::Alerts(
                                                                            None,
                                                                            None,
                                                                            Some(mute_time),
                                                                        ),
                                                                        message.sender.to_string(),
                                                                    ),
                                                                )
                                                            }
                                                            None => {
                                                                match other_params.split_once(' ') {
                                                                    None => {
                                                                        // !subscribe alerts turboflakes
                                                                        commands.push(Commands::Subscribe(
                                                                ReportType::Alerts(
                                                                    Some(other_params.to_string()),
                                                                    None,
                                                                    None,
                                                                ),
                                                                message.sender.to_string(),
                                                            ))
                                                                    }
                                                                    Some((
                                                                        member,
                                                                        other_params,
                                                                    )) => {
                                                                        match extract_mute_time(other_params) {
                                                                Some(mute_time) => {
                                                                    // !subscribe alerts turboflakes [10]
                                                                    commands.push(
                                                                        Commands::Subscribe(
                                                                            ReportType::Alerts(
                                                                                Some(
                                                                                    member
                                                                                        .to_string(
                                                                                        ),
                                                                                ),
                                                                                None,
                                                                                Some(mute_time),
                                                                            ),
                                                                            message
                                                                                .sender
                                                                                .to_string(),
                                                                        ),
                                                                    )
                                                                }
                                                                None => match other_params
                                                                    .split_once(' ')
                                                                {
                                                                    Some((
                                                                        severity,
                                                                        other_params,
                                                                    )) => match extract_mute_time(
                                                                        other_params,
                                                                    ) {
                                                                        Some(mute_time) => {
                                                                            // !subscribe alerts turboflakes high [10]
                                                                            commands.push(Commands::Subscribe(
                                                                            ReportType::Alerts(
                                                                                Some(member.to_string()),
                                                                                Some(severity.into()),
                                                                                Some(mute_time),
                                                                            ),
                                                                            message.sender.to_string(),
                                                                        ))
                                                                        }
                                                                        None => commands.push(
                                                                            Commands::NotSupported,
                                                                        ),
                                                                    },
                                                                    None => {
                                                                        // !subscribe alerts turboflakes high
                                                                        commands.push(Commands::Subscribe(
                                                                    ReportType::Alerts(
                                                                        Some(member.to_string()),
                                                                        Some(other_params.into()),
                                                                        None,
                                                                    ),
                                                                    message.sender.to_string(),
                                                                ))
                                                                    }
                                                                },
                                                            }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    _ => commands.push(Commands::NotSupported),
                                                }
                                            }
                                        },
                                        "!unsubscribe" => match other_params.split_once(' ') {
                                            None => match other_params {
                                                "alerts" => {
                                                    // !unsubscribe alerts
                                                    commands.push(Commands::UnsubscribeAll(
                                                        ReportType::Alerts(None, None, None),
                                                        message.sender.to_string(),
                                                    ))
                                                }
                                                _ => commands.push(Commands::NotSupported),
                                            },
                                            Some((report_type, other_params)) => {
                                                match report_type {
                                                    "alerts" => {
                                                        match other_params.split_once(' ') {
                                                            None => {
                                                                // !unsubscribe alerts turboflakes
                                                                commands.push(
                                                                    Commands::Unsubscribe(
                                                                        ReportType::Alerts(
                                                                            Some(
                                                                                other_params
                                                                                    .to_string(),
                                                                            ),
                                                                            None,
                                                                            None,
                                                                        ),
                                                                        message.sender.to_string(),
                                                                    ),
                                                                )
                                                            }
                                                            Some((member, other_params)) => {
                                                                // !unsubscribe alerts turboflakes high
                                                                commands.push(
                                                                    Commands::Unsubscribe(
                                                                        ReportType::Alerts(
                                                                            Some(
                                                                                member.to_string(),
                                                                            ),
                                                                            Some(
                                                                                other_params.into(),
                                                                            ),
                                                                            None,
                                                                        ),
                                                                        message.sender.to_string(),
                                                                    ),
                                                                )
                                                            }
                                                        }
                                                    }
                                                    _ => commands.push(Commands::NotSupported),
                                                }
                                            }
                                        },
                                        _ => commands.push(Commands::NotSupported),
                                    },
                                };
                            }
                        }
                        // Cache next token
                        let next_token = if events.end == "" {
                            events.start
                        } else {
                            events.end
                        };
                        fs::write(&next_token_filename, next_token)?;
                        Ok(Some(commands))
                    }
                    _ => {
                        let response = res.json::<ErrorResponse>().await?;
                        Err(MatrixError::Other(response.error))
                    }
                }
            }
            None => Err(MatrixError::Other("access_token not defined".to_string())),
        }
    }

    // Getting events for a room
    // https://spec.matrix.org/v1.2/client-server-api/#get_matrixclientv3roomsroomidmessages
    async fn get_members_from_room_and_token(
        &self,
        room_id: &str,
    ) -> Result<Option<Vec<UserID>>, MatrixError> {
        match &self.access_token {
            Some(access_token) => {
                let config = CONFIG.clone();
                let next_token_filename = format!(
                    "{}{}.members.{}",
                    config.data_path, MATRIX_NEXT_TOKEN_FILENAME, room_id
                );
                let client = self.client.clone();
                let room_id_encoded: String = byte_serialize(room_id.as_bytes()).collect();
                let filter = RoomEventFilter {
                    types: vec!["m.room.member".to_string()],
                    rooms: vec![room_id.to_string()],
                };
                let filter_str = serde_json::to_string(&filter)?;
                let filter_encoded: String = byte_serialize(filter_str.as_bytes()).collect();

                // Try to read first cached next token from file
                let url = match fs::read_to_string(&next_token_filename) {
                    Ok(next_token) => format!(
                        "{}/rooms/{}/messages?access_token={}&from={}&filter={}",
                        MATRIX_URL, room_id_encoded, access_token, next_token, filter_encoded
                    ),
                    _ => format!(
                        "{}/rooms/{}/messages?access_token={}&filter={}",
                        MATRIX_URL, room_id_encoded, access_token, filter_encoded
                    ),
                };

                let res = client.get(url).send().await?;
                match res.status() {
                    reqwest::StatusCode::OK => {
                        let events = res.json::<RoomEventsResponse>().await?;
                        let mut members: Vec<UserID> = Vec::new();
                        // Parse message to commands
                        for message in events.chunk.iter() {
                            // skip bot user
                            if message.content.membership == "join"
                                && message.user_id != config.matrix_bot_user
                            {
                                members.push(message.user_id.to_string());
                            }
                        }
                        // Cache next token
                        let next_token = if events.end == "" {
                            events.start
                        } else {
                            events.end
                        };
                        fs::write(&next_token_filename, next_token)?;
                        Ok(Some(members))
                    }
                    _ => {
                        let response = res.json::<ErrorResponse>().await?;
                        Err(MatrixError::Other(response.error))
                    }
                }
            }
            None => Err(MatrixError::Other("access_token not defined".to_string())),
        }
    }

    // Getting members for a room
    // https://spec.matrix.org/v1.2/client-server-api/#get_matrixclientv3roomsroomidmembers
    async fn get_members_from_room(&self, room_id: &str) -> Result<HashSet<UserID>, MatrixError> {
        match &self.access_token {
            Some(access_token) => {
                let config = CONFIG.clone();
                let client = self.client.clone();
                let room_id_encoded: String = byte_serialize(room_id.as_bytes()).collect();
                let res = client
                    .get(format!(
                        "{}/rooms/{}/members?access_token={}&membership=join",
                        MATRIX_URL, room_id_encoded, access_token
                    ))
                    .send()
                    .await?;
                match res.status() {
                    reqwest::StatusCode::OK => {
                        let events = res.json::<RoomEventsResponse>().await?;
                        let mut members: HashSet<UserID> = HashSet::new();
                        // Parse message to members
                        for message in events.chunk.iter() {
                            // skip bot user
                            if message.content.membership == "join"
                                && message.user_id != config.matrix_bot_user
                            {
                                members.insert(message.user_id.to_string());
                            }
                        }
                        Ok(members)
                    }
                    _ => {
                        let response = res.json::<ErrorResponse>().await?;
                        Err(MatrixError::Other(response.error))
                    }
                }
            }
            None => Err(MatrixError::Other("access_token not defined".to_string())),
        }
    }

    #[async_recursion]
    async fn join_room(&self, room_id: &str) -> Result<Option<RoomID>, MatrixError> {
        match &self.access_token {
            Some(access_token) => {
                let client = self.client.clone();
                let room_id_encoded: String = byte_serialize(room_id.as_bytes()).collect();
                let res = client
                    .post(format!(
                        "{}/join/{}?access_token={}",
                        MATRIX_URL, room_id_encoded, access_token
                    ))
                    .send()
                    .await?;
                debug!("response {:?}", res);
                match res.status() {
                    reqwest::StatusCode::OK => {
                        let room = res.json::<Room>().await?;
                        info!("The room {} has been joined.", room.room_id);
                        Ok(Some(room.room_id))
                    }
                    reqwest::StatusCode::TOO_MANY_REQUESTS => {
                        let response = res.json::<ErrorResponse>().await?;
                        warn!("Matrix {} -> Wait 5 seconds and try again", response.error);
                        thread::sleep(time::Duration::from_secs(5));
                        return self.join_room(room_id).await;
                    }
                    _ => {
                        let response = res.json::<ErrorResponse>().await?;
                        Err(MatrixError::Other(response.error))
                    }
                }
            }
            None => Err(MatrixError::Other("access_token not defined".to_string())),
        }
    }

    pub async fn reply_help(&self, room_id: &str) -> Result<(), MatrixError> {
        let mut message = String::from("✨ Supported commands:<br>");
        message.push_str("<b>!subscribe alerts [MUTE_INTERVAL]</b> - Subscribe to All IBP-monitor alerts from all members. The parameter MUTE_INTERVAL is optional and is defined in minutes, e.g 10.<br>");
        message.push_str("<b>!subscribe alerts <i>MEMBER</i> [MUTE_INTERVAL]</b> - Subscribe to IBP-monitor alerts by MEMBER.<br>");
        message.push_str("<b>!subscribe alerts <i>MEMBER</i> <i>SEVERITY</i> [MUTE_INTERVAL]</b> - Subscribe to IBP-monitor alerts by MEMBER and SEVERITY. The parameter SEVERITY must match one of the options: [high, medium, low].<br>");

        message.push_str("<b>!unsubscribe alerts</b> - Unsubscribe to All IBP-monitor alerts.<br>");
        message.push_str(
            "<b>!unsubscribe alerts <i>MEMBER</i></b> - Unsubscribe to IBP-monitor alerts by MEMBER.<br>",
        );
        message.push_str(
            "<b>!unsubscribe alerts <i>MEMBER</i> <i>SEVERITY</i></b> - Unsubscribe to IBP-monitor alerts by MEMBER and SEVERITY.<br>",
        );

        message.push_str("<b>!help</b> - Print this message.<br>");
        message.push_str("——<br>");
        message.push_str(&format!(
            "<code>{} v{}</code><br>",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        ));

        return self
            .send_room_message(&room_id, &message, Some(&message))
            .await;
    }

    async fn send_room_message(
        &self,
        room_id: &str,
        message: &str,
        formatted_message: Option<&str>,
    ) -> Result<(), MatrixError> {
        if self.disabled {
            return Ok(());
        }
        let req = SendRoomMessageRequest::with_message(&message, formatted_message);
        self.dispatch_message(&room_id, &req).await?;
        Ok(())
    }

    pub async fn send_private_message(
        &self,
        to_user_id: &str,
        message: &str,
        formatted_message: Option<&str>,
    ) -> Result<(), MatrixError> {
        if self.disabled {
            return Ok(());
        }
        // Get or create user private room
        if let Some(private_room) = self.get_or_create_private_room(to_user_id).await? {
            // Send message to the private room (bot <=> user)
            let req = SendRoomMessageRequest::with_message(&message, formatted_message);
            self.dispatch_message(&private_room.room_id, &req).await?;
        }

        Ok(())
    }

    pub async fn send_public_message(
        &self,
        message: &str,
        formatted_message: Option<&str>,
    ) -> Result<(), MatrixError> {
        if self.disabled {
            return Ok(());
        }
        let config = CONFIG.clone();
        // Send message to public room (public room available for the connected chain)
        if !config.matrix_public_room_disabled {
            let req = SendRoomMessageRequest::with_message(&message, formatted_message);
            self.dispatch_message(&self.public_room_id, &req).await?;
        }

        Ok(())
    }

    pub async fn send_callout_message(
        &self,
        message: &str,
        formatted_message: Option<&str>,
    ) -> Result<(), MatrixError> {
        if self.disabled {
            return Ok(());
        }
        let config = CONFIG.clone();
        // Send message to callout public rooms
        if !config.matrix_public_room_disabled {
            for room_id in self.callout_public_room_ids.iter() {
                let req = SendRoomMessageRequest::with_message(&message, formatted_message);
                self.dispatch_message(&room_id, &req).await?;
            }
        }

        Ok(())
    }

    pub async fn send_private_file(
        &self,
        to_user_id: &str,
        filename: &str,
        url: &str,
        file_info: Option<FileInfo>,
    ) -> Result<(), MatrixError> {
        if self.disabled {
            return Ok(());
        }
        // Get or create user private room
        if let Some(private_room) = self.get_or_create_private_room(to_user_id).await? {
            // Send message to the private room (bot <=> user)
            let req = SendRoomMessageRequest::with_attachment(&filename, &url, file_info);
            self.dispatch_message(&private_room.room_id, &req).await?;
        }

        Ok(())
    }

    #[async_recursion]
    async fn dispatch_message(
        &self,
        room_id: &str,
        request: &SendRoomMessageRequest,
    ) -> Result<Option<EventID>, MatrixError> {
        if self.disabled {
            return Ok(None);
        }
        match &self.access_token {
            Some(access_token) => {
                let client = self.client.clone();
                let res = client
                    .post(format!(
                        "{}/rooms/{}/send/m.room.message?access_token={}",
                        MATRIX_URL, room_id, access_token
                    ))
                    .json(request)
                    .send()
                    .await?;

                debug!("response {:?}", res);
                match res.status() {
                    reqwest::StatusCode::OK => {
                        let response = res.json::<SendRoomMessageResponse>().await?;
                        info!(
                            "messsage dispatched to room_id: {} (event_id: {})",
                            room_id, response.event_id
                        );
                        Ok(Some(response.event_id))
                    }
                    reqwest::StatusCode::TOO_MANY_REQUESTS => {
                        let response = res.json::<ErrorResponse>().await?;
                        warn!("Matrix {} -> Wait 5 seconds and try again", response.error);
                        thread::sleep(time::Duration::from_secs(5));
                        return self.dispatch_message(room_id, request).await;
                    }
                    _ => {
                        let response = res.json::<ErrorResponse>().await?;
                        Err(MatrixError::Other(response.error))
                    }
                }
            }
            None => Err(MatrixError::Other("access_token not defined".to_string())),
        }
    }
}

pub async fn add_matrix(cfg: &mut web::ServiceConfig) {
    let mut matrix: Matrix = Matrix::new();
    matrix.authenticate().await.unwrap_or_else(|_e| {
        // error!("{}", e);
        Default::default()
    });
    // let pool = create_pool(CONFIG.clone()).expect("failed to create Redis pool");
    cfg.app_data(web::Data::new(matrix));
}

fn extract_mute_time(input: &str) -> Option<u32> {
    if let Ok(n) = input.trim_start_matches("[").trim_end_matches("]").parse() {
        return Some(n);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_mute_time_from_str() {
        assert_eq!(extract_mute_time("[123]"), Some(123));
        assert_eq!(extract_mute_time("123]"), Some(123));
        assert_eq!(extract_mute_time("12e3]"), None);
    }
}
