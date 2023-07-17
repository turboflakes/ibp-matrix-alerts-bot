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

use crate::abot::{HealthCheckId, MaintenanceMode, MemberId, ServiceId, Severity};
use crate::api::helpers::respond_json;
use crate::cache::{get_conn, CacheKey};
use crate::matrix::UserID;
use std::collections::HashMap;
// use crate::config::CONFIG;
use crate::errors::{ApiError, CacheError};
use crate::report::{RawAlert, Report};
use crate::Abot;
use actix_web::{web, web::Json};
use chrono::Utc;
use redis::aio::Connection;
use serde::{Deserialize, Serialize};
use serde_json::value::Value;

const WHITELIST_SERVICES: [&'static str; 12] = [
    "polkadot-rpc",
    "kusama-rpc",
    "westend-rpc",
    "statemint-rpc",
    "statemine-rpc",
    "westmint-rpc",
    "collectives-polkadot-rpc",
    "collectives-westend-rpc",
    "bridgehub-polkadot-rpc",
    "bridgehub-kusama-rpc",
    "bridgehub-westend-rpc",
    "encointer-kusama-rpc",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Delivered,
    _Skipped,
}

#[derive(Debug, Serialize)]
pub struct Response {
    data: Vec<(UserID, Status)>,
}

// #[allow(dead_code)]
// #[derive(Debug, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct HealthCheckRecord {
//     monitor_id: String,
//     service_id: String,
//     member_id: String,
//     endpoint: String,
//     ip_address: String,
//     chain: String,
//     version: String,
//     performance: f64,
// }

// #[allow(dead_code)]
// #[derive(Debug, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct HealthCheck {
//     monitor_id: String,
//     service_id: String,
//     member_id: String,
//     peer_id: String,
//     source: String,
//     r#type: String,
//     status: String,
//     response_time_ms: f64,
//     record: HealthCheckRecord,
// }

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    code: u32,
    severity: Severity,
    message: String,
    member_id: MemberId,
    service_id: ServiceId,
    health_check_id: HealthCheckId,
    health_checks: Vec<Value>,
}

/// Handler to receive new alerts from monitor
pub async fn post_alert(
    new_alert: web::Json<Alert>,
    abot: web::Data<Abot>,
) -> Result<Json<Response>, ApiError> {
    let mut conn = get_conn(&abot.cache).await?;

    // get maintenance status for the member in the alert
    let maintenance_mode = redis::cmd("HGET")
        .arg(CacheKey::Maintenance(new_alert.member_id.to_string()))
        .arg("mode".to_string())
        .query_async::<Connection, MaintenanceMode>(&mut conn)
        .await
        .map_err(CacheError::RedisCMDError)?;

    // if maintenance is active for the member skip alerts
    if maintenance_mode == MaintenanceMode::On {
        return respond_json(Response { data: vec![] });
    }

    // get all subscribers for the type of alert received by member and severity
    let subscribers = redis::cmd("SMEMBERS")
        .arg(CacheKey::Subscribers(
            new_alert.member_id.to_string(),
            new_alert.severity.clone(),
        ))
        .query_async::<Connection, Vec<UserID>>(&mut conn)
        .await
        .map_err(CacheError::RedisCMDError)?;

    let mut resp_data: Vec<(UserID, Status)> = Vec::new();

    for subscriber in subscribers {
        // get last time the same alert code:service as been sent
        let key = format!(
            "{}:{}",
            new_alert.code.to_string(),
            new_alert.service_id.to_string()
        );
        let exists = redis::cmd("HEXISTS")
            .arg(CacheKey::LastAlerts(
                subscriber.to_string(),
                new_alert.member_id.to_string(),
            ))
            .arg(&key)
            .query_async::<Connection, bool>(&mut conn)
            .await
            .map_err(CacheError::RedisCMDError)?;

        let last_time_sent = if exists {
            redis::cmd("HGET")
                .arg(CacheKey::LastAlerts(
                    subscriber.to_string(),
                    new_alert.member_id.to_string(),
                ))
                .arg(&key)
                .query_async::<Connection, i64>(&mut conn)
                .await
                .map_err(CacheError::RedisCMDError)?
        } else {
            0
        };

        // get mute time defined by the user
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

        // send alert and update last_alert timestamp
        let now = Utc::now();
        if now.timestamp() > last_time_sent + (mute_time * 60)
            && WHITELIST_SERVICES.contains(&&new_alert.service_id[..])
        {
            let record_serialized = serde_json::to_string(&new_alert.health_checks)?;

            let report = Report::from(RawAlert {
                code: new_alert.code,
                member_id: new_alert.member_id.to_owned(),
                service_id: new_alert.service_id.to_owned(),
                health_check_id: new_alert.health_check_id.to_owned(),
                severity: new_alert.severity.clone(),
                message: new_alert.message.to_owned(),
                data: record_serialized,
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
            let data = HashMap::from([
                (new_alert.code.to_string(), now.timestamp().to_string()),
                (key, now.timestamp().to_string()),
            ]);
            redis::cmd("HSET")
                .arg(CacheKey::LastAlerts(
                    subscriber.to_string(),
                    new_alert.member_id.to_string(),
                ))
                .arg(data)
                .query_async::<Connection, _>(&mut conn)
                .await
                .map_err(CacheError::RedisCMDError)?;

            resp_data.push((subscriber, Status::Delivered));
        }
    }

    let now = Utc::now();
    // increment alert code counter
    redis::cmd("HINCRBY")
        .arg(CacheKey::StatsByCode(
            now.format("%y%m%d").to_string(),
            new_alert.member_id.to_string(),
        ))
        .arg(new_alert.code.to_string())
        .arg(1)
        .query_async::<Connection, _>(&mut conn)
        .await
        .map_err(CacheError::RedisCMDError)?;

    // increment alert severity counter
    redis::cmd("HINCRBY")
        .arg(CacheKey::StatsBySeverity(
            now.format("%y%m%d").to_string(),
            new_alert.member_id.to_string(),
        ))
        .arg(new_alert.severity.to_string())
        .arg(1)
        .query_async::<Connection, _>(&mut conn)
        .await
        .map_err(CacheError::RedisCMDError)?;

    // increment alert service counter
    redis::cmd("HINCRBY")
        .arg(CacheKey::StatsByService(
            now.format("%y%m%d").to_string(),
            new_alert.member_id.to_string(),
        ))
        .arg(new_alert.service_id.to_string())
        .arg(1)
        .query_async::<Connection, _>(&mut conn)
        .await
        .map_err(CacheError::RedisCMDError)?;

    respond_json(Response { data: resp_data })
}
