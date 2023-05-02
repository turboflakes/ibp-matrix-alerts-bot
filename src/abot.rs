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

use crate::cache::{create_or_await_pool, RedisPool, get_conn, CacheKey};
use crate::config::{Config, CONFIG};
use crate::errors::{AbotError, CacheError};
use crate::matrix::{Matrix, UserID};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::mem;
use std::{convert::TryInto, result::Result, thread, time};
use reqwest::Url;
use std::{collections::HashMap};
use redis::aio::Connection;

#[derive(Clone)]
pub struct Abot {
    matrix: Matrix,
    pub cache: RedisPool,
}

impl Abot {
    pub async fn new() -> Abot {
        // Initialize matrix client
        let mut matrix: Matrix = Matrix::new();
        matrix.authenticate().await.unwrap_or_else(|e| {
            error!("{}", e);
            Default::default()
        });

        Abot {
            matrix,
            cache: create_or_await_pool(CONFIG.clone()),
        }
    }

    /// Returns the matrix configuration
    pub fn matrix(&self) -> &Matrix {
        &self.matrix
    }

    /// Spawn and restart on error
    pub fn start() {

        // Fetch and cache member Ids
        spawn_and_fetch_members_from_remote_url();

        // Authenticate matrix and spawn lazy load commands
        spawn_and_restart_matrix_lazy_load_on_error();
    }
}

// spawns a task to fetch and cache member ids from remote config file
fn spawn_and_fetch_members_from_remote_url() {
    async_std::task::spawn(async {
        if let Err(e) = try_fetch_members_from_remote_url().await {
            error!("fetch members error: {}", e);
        }
    });
}

// spawns a task to load and process commands from matrix
fn spawn_and_restart_matrix_lazy_load_on_error() {
    async_std::task::spawn(async {

        let config = CONFIG.clone();
        if !config.matrix_disabled {
            loop {
                let mut m = Matrix::new();
                if let Err(e) = m.authenticate().await {
                    error!("authenticate error: {}", e);
                    thread::sleep(time::Duration::from_secs(config.error_interval));
                    continue;
                }
                if let Err(e) = m.lazy_load_and_process_commands().await {
                    error!("lazy_load_and_process_commands error: {}", e);
                    thread::sleep(time::Duration::from_secs(config.error_interval));
                    continue;
                }
            }
        }
    });
}

pub type MemberId = String;

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    High,
    Medium,
    Low,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
            Self::Low => write!(f, "low"),
        }
    }
}

impl Default for Severity {
    fn default() -> Self {
        Severity::High
    }
}

impl From<Severity> for String {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::High => "".to_string(),
            _ => "".to_string(),
        }
        // ErrorResponse {
        //     errors: vec![error.into()],
        // }
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub enum ReportType {
    Alerts(Option<(MemberId, Severity)>),
}

impl ReportType {
    pub fn name(&self) -> String {
        match &self {
            Self::Alerts(Some((member_id, severity))) => {
                format!("Alerts from {} with {} severity", member_id, severity)
            }
            Self::Alerts(None) => "Alerts format not supported".to_string(),
        }
    }
}

impl std::fmt::Display for ReportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Alerts(_option) => write!(f, "Alerts"),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct MembersResponse {
    members: HashMap<MemberId, serde_json::Value>
}
/// Fetch members from ibp-monitor main repo https://raw.githubusercontent.com/ibp-network/config/main/members.json
pub async fn try_fetch_members_from_remote_url(
) -> Result<(), AbotError> {
    let config = CONFIG.clone();
    if config.members_json_url.len() == 0 {
        return Err(AbotError::Other("config.members_json_url not specified".to_string()))
    }

    let url = Url::parse(&*config.members_json_url)?;
    match reqwest::get(url.to_string()).await {
        Ok(response) => {
            match response.json::<MembersResponse>().await {
                Ok(data) => {
                    // cache members
                    let cache = create_or_await_pool(CONFIG.clone());
                    let mut conn = get_conn(&cache).await?;
                    for (member, _) in data.members {
                        redis::cmd("SADD")
                                    .arg(CacheKey::Members)
                                    .arg(member.to_string())
                                    .query_async::<Connection, bool>(&mut conn)
                                    .await
                                    .map_err(CacheError::RedisCMDError)?;
                    }

                }
                Err(e) => {
                    return Err(AbotError::ReqwestError(e))
                }
            }
        }
        Err(e) => {
            return Err(AbotError::ReqwestError(e))
        }
    }
    Ok(())
}

