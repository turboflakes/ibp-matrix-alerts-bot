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

use crate::abot::{MemberId, Severity, Who};
use crate::api::helpers::respond_json;
use crate::cache::{get_conn, CacheKey};
use crate::config::CONFIG;
use crate::errors::{ApiError, CacheError};
use crate::Abot;
use actix_web::{web, web::Json};
use chrono::Utc;
use log::{error, info};
use redis::aio::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Processed,
    Skipped,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub status: Status,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckRecord {
    monitor_id: String,
    service_id: String,
    member_id: String,
    endpoint: String,
    ip_address: String,
    chain: String,
    version: String,
    performance: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    monitor_id: String,
    service_id: String,
    member_id: String,
    peer_id: String,
    source: String,
    r#type: String,
    status: String,
    response_time_ms: f64,
    record: HealthCheckRecord,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    code: u32,
    severity: Severity,
    message: String,
    member_id: MemberId,
    health_checks: Vec<HealthCheck>,
}

/// Handler to receive new alerts from monitor
pub async fn post_alert(
    new_alert: web::Json<Alert>,
    abot: web::Data<Abot>,
) -> Result<Json<Response>, ApiError> {
    // let config = CONFIG.clone();
    let mut conn = get_conn(&abot.cache).await?;

    // 1st. get all subscribers for the type of alert received by member and severity
    let subscribers = redis::cmd("SMEMBERS")
        .arg(CacheKey::Subscribers(
            new_alert.member_id.to_string(),
            new_alert.severity.clone(),
        ))
        .query_async::<Connection, Vec<Who>>(&mut conn)
        .await
        .map_err(CacheError::RedisCMDError)?;

    for subscriber in subscribers {
        // 2nd. get last time the same alert code as been sent
        let exists = redis::cmd("HEXISTS")
            .arg(CacheKey::LastAlerts(
                subscriber.to_string(),
                new_alert.member_id.to_string(),
            ))
            .arg(new_alert.code.to_string())
            .query_async::<Connection, bool>(&mut conn)
            .await
            .map_err(CacheError::RedisCMDError)?;

        let last_time_sent = if exists {
            redis::cmd("HGET")
                .arg(CacheKey::LastAlerts(
                    subscriber.to_string(),
                    new_alert.member_id.to_string(),
                ))
                .arg(new_alert.code.to_string())
                .query_async::<Connection, i64>(&mut conn)
                .await
                .map_err(CacheError::RedisCMDError)?
        } else {
            0
        };

        // 3rd get mute time defined by the user
        let mute_time = redis::cmd("HGET")
            .arg(CacheKey::SubscriberConfig(
                subscriber.to_string(),
                new_alert.member_id.to_string(),
                new_alert.severity.clone(),
            ))
            .arg("mute".to_string())
            .query_async::<Connection, i64>(&mut conn)
            .await
            .map_err(CacheError::RedisCMDError)?;

        // 4th send alert and update last_alert timestamp
        let now = Utc::now();
        if now.timestamp() > last_time_sent + (mute_time * 60) {
            let _ = &abot
                .matrix()
                .send_public_message(&new_alert.message, Some(&new_alert.message))
                .await?;
            //
            redis::cmd("HSET")
                .arg(CacheKey::LastAlerts(
                    subscriber.to_string(),
                    new_alert.member_id.to_string(),
                ))
                .arg(new_alert.code.to_string())
                .arg(now.timestamp().to_string())
                .query_async::<Connection, i64>(&mut conn)
                .await
                .map_err(CacheError::RedisCMDError)?;

            return respond_json(Response {
                status: Status::Processed,
            });
        }
    }

    respond_json(Response {
        status: Status::Skipped,
    })
}
