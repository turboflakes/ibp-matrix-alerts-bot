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

use crate::abot::{MemberId, ServiceId, Severity, Who};
use crate::api::helpers::respond_json;
use crate::cache::{get_conn, CacheKey};
// use crate::config::CONFIG;
use crate::errors::{ApiError, CacheError};
use crate::report::{RawAlert, Report};
use crate::Abot;
use actix_web::{web, web::Json};
use chrono::Utc;
use redis::aio::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Delivered,
    Skipped,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub status: Status,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    code: u32,
    severity: Severity,
    message: String,
    member_id: MemberId,
    service_id: ServiceId,
    health_checks: Vec<HealthCheck>,
}

/// Handler to receive new alerts from monitor
pub async fn post_alert(
    new_alert: web::Json<Alert>,
    abot: web::Data<Abot>,
) -> Result<Json<Response>, ApiError> {
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
            let report = Report::from(RawAlert {
                code: new_alert.code,
                member_id: new_alert.member_id.to_owned(),
                service_id: new_alert.service_id.to_owned(),
                severity: new_alert.severity.clone(),
                message: new_alert.message.to_owned(),
            });

            let _ = &abot
                .matrix()
                .send_private_message(
                    &subscriber,
                    &report.message(),
                    Some(&report.formatted_message()),
                )
                .await?;

            //
            redis::cmd("HSET")
                .arg(CacheKey::LastAlerts(
                    subscriber.to_string(),
                    new_alert.member_id.to_string(),
                ))
                .arg(new_alert.code.to_string())
                .arg(now.timestamp().to_string())
                .query_async::<Connection, _>(&mut conn)
                .await
                .map_err(CacheError::RedisCMDError)?;

            return respond_json(Response {
                status: Status::Delivered,
            });
        }
    }

    // 5th increment alert code counter
    redis::cmd("HINCRBY")
        .arg(CacheKey::StatsByCode(new_alert.member_id.to_string()))
        .arg(new_alert.code.to_string())
        .arg(1)
        .query_async::<Connection, _>(&mut conn)
        .await
        .map_err(CacheError::RedisCMDError)?;

    // 6th increment alert severity counter
    redis::cmd("HINCRBY")
        .arg(CacheKey::StatsBySeverity(new_alert.member_id.to_string()))
        .arg(new_alert.severity.to_string())
        .arg(1)
        .query_async::<Connection, _>(&mut conn)
        .await
        .map_err(CacheError::RedisCMDError)?;

    respond_json(Response {
        status: Status::Skipped,
    })
}
