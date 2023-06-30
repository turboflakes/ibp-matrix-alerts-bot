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

use crate::config::CONFIG;

use std::{result::Result, sync::mpsc, thread, time};

use log::{error, info, warn};
use rust_socketio::{ClientBuilder, Payload, RawClient, TransportType};
use serde::{
    de::{Deserializer, MapAccess, Visitor},
    Deserialize, Serialize,
};
use serde_json::json;
use std::time::Duration;

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Check,
    Gossip,
}

impl Default for Source {
    fn default() -> Self {
        Source::Gossip
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Check => write!(f, "check"),
            Self::Gossip => write!(f, "gossip"),
        }
    }
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Success,
    Warning,
    Error,
}

impl Default for Status {
    fn default() -> Self {
        Status::Error
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success => write!(f, "success"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
        }
    }
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Type {
    ServiceCheck,
    SystemHealth,
    BestBlock,
}

impl Default for Type {
    fn default() -> Self {
        Type::ServiceCheck
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ServiceCheck => write!(f, "service_check"),
            Self::SystemHealth => write!(f, "system_health"),
            Self::BestBlock => write!(f, "best_block"),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    peers: u32,
    is_syncing: bool,
    should_have_peers: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    starting_block: u32,
    current_block: u32,
    highest_block: u32,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveState {
    random_block: u32,
    spec_version: String,
}

#[derive(Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChainType {
    live: Option<String>,
}

#[derive(Debug, Default)]
pub struct Record {
    monitor_id: String,
    service_id: String,
    member_id: String,
    endpoint: String,
    ip_address: String,
    chain: String,
    chain_type: ChainType,
    health: Health,
    sync_state: SyncState,
    finalized_block: u32,
    archive_state: ArchiveState,
    version: String,
    performance: f64,
}

// https://serde.rs/deserialize-struct.html
// NOTE: HealthCheck is manually deserialized because some of the fields might not exist
// and rather than rasing error just set default values
//
impl<'de> Deserialize<'de> for Record {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize, Debug)]
        #[serde(field_identifier, rename_all = "camelCase")]
        enum Field {
            MonitorId,
            ServiceId,
            MemberId,
            Endpoint,
            IpAddress,
            Chain,
            ChainType,
            Health,
            SyncState,
            FinalizedBlock,
            ArchiveState,
            Version,
            Performance,
        }

        struct RecordVisitor;

        impl<'de> Visitor<'de> for RecordVisitor {
            type Value = Record;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct Record")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Record, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut monitor_id: Option<String> = None;
                let mut service_id: Option<String> = None;
                let mut member_id: Option<String> = None;
                let mut endpoint: Option<String> = None;
                let mut ip_address: Option<String> = None;
                let mut chain: Option<String> = None;
                let mut chain_type: Option<ChainType> = None;
                let mut health: Option<Health> = None;
                let mut sync_state: Option<SyncState> = None;
                let mut finalized_block: Option<u32> = None;
                let mut archive_state: Option<ArchiveState> = None;
                let mut version: Option<String> = None;
                let mut performance: Option<f64> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::MonitorId => {
                            if monitor_id.is_some() {
                                return Err(serde::de::Error::duplicate_field("monitor_id"));
                            }
                            monitor_id = Some(map.next_value()?);
                        }
                        Field::ServiceId => {
                            if service_id.is_some() {
                                return Err(serde::de::Error::duplicate_field("service_id"));
                            }
                            service_id = Some(map.next_value()?);
                        }
                        Field::MemberId => {
                            if member_id.is_some() {
                                return Err(serde::de::Error::duplicate_field("member_id"));
                            }
                            member_id = Some(map.next_value()?);
                        }
                        Field::Endpoint => {
                            if endpoint.is_some() {
                                return Err(serde::de::Error::duplicate_field("endpoint"));
                            }
                            endpoint = Some(map.next_value()?);
                        }
                        Field::IpAddress => {
                            if ip_address.is_some() {
                                return Err(serde::de::Error::duplicate_field("ip_address"));
                            }
                            ip_address = Some(map.next_value()?);
                        }
                        Field::Chain => {
                            if chain.is_some() {
                                return Err(serde::de::Error::duplicate_field("chain"));
                            }
                            chain = Some(map.next_value()?);
                        }
                        Field::ChainType => {
                            if chain_type.is_some() {
                                return Err(serde::de::Error::duplicate_field("chain_type"));
                            }
                            chain_type = Some(map.next_value()?);
                        }
                        Field::Health => {
                            if health.is_some() {
                                return Err(serde::de::Error::duplicate_field("health"));
                            }
                            health = Some(map.next_value()?);
                        }
                        Field::SyncState => {
                            if sync_state.is_some() {
                                return Err(serde::de::Error::duplicate_field("sync_state"));
                            }
                            sync_state = Some(map.next_value()?);
                        }
                        Field::FinalizedBlock => {
                            if finalized_block.is_some() {
                                return Err(serde::de::Error::duplicate_field("finalized_block"));
                            }
                            finalized_block = Some(map.next_value()?);
                        }
                        Field::ArchiveState => {
                            if archive_state.is_some() {
                                return Err(serde::de::Error::duplicate_field("archive_state"));
                            }
                            archive_state = Some(map.next_value()?);
                        }
                        Field::Version => {
                            if version.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version = Some(map.next_value()?);
                        }
                        Field::Performance => {
                            if performance.is_some() {
                                return Err(serde::de::Error::duplicate_field("performance"));
                            }
                            performance = Some(map.next_value()?);
                        }
                    }
                }
                let monitor_id = monitor_id.unwrap_or_default();
                let service_id = service_id.unwrap_or_default();
                let member_id = member_id.unwrap_or_default();
                let endpoint = endpoint.unwrap_or_default();
                let ip_address = ip_address.unwrap_or_default();
                let chain = chain.unwrap_or_default();
                let chain_type = chain_type.unwrap_or_default();
                let health = health.unwrap_or_default();
                let sync_state = sync_state.unwrap_or_default();
                let finalized_block = finalized_block.unwrap_or_default();
                let archive_state = archive_state.unwrap_or_default();
                let version = version.unwrap_or_default();
                let performance = performance.unwrap_or_default();

                Ok(Record::new(
                    monitor_id,
                    service_id,
                    member_id,
                    endpoint,
                    ip_address,
                    chain,
                    chain_type,
                    health,
                    sync_state,
                    finalized_block,
                    archive_state,
                    version,
                    performance,
                ))
            }
        }

        const FIELDS: &'static [&'static str] = &[
            "monitor_id",
            "service_id",
            "member_id",
            "endpoint",
            "ip_address",
            "chain",
            "chain_type",
            "health",
            "sync_state",
            "finalized_block",
            "archive_state",
            "version",
            "performance",
        ];
        deserializer.deserialize_struct("Record", FIELDS, RecordVisitor)
    }
}

impl Record {
    pub fn new(
        monitor_id: String,
        service_id: String,
        member_id: String,
        endpoint: String,
        ip_address: String,
        chain: String,
        chain_type: ChainType,
        health: Health,
        sync_state: SyncState,
        finalized_block: u32,
        archive_state: ArchiveState,
        version: String,
        performance: f64,
    ) -> Self {
        Self {
            monitor_id,
            service_id,
            member_id,
            endpoint,
            ip_address,
            chain,
            chain_type,
            health,
            sync_state,
            finalized_block,
            archive_state,
            version,
            performance,
        }
    }
}

#[derive(Debug, Default)]
pub struct HealthCheck {
    id: u32,
    monitor_id: String,
    service_id: String,
    member_id: String,
    peer_id: String,
    source: Source,
    r#type: Type,
    status: Status,
    response_time_ms: f64,
    created_at: String,
    record: Record,
}

// https://serde.rs/deserialize-struct.html
// NOTE: HealthCheck is manually deserialized because some of the fields might not exist
// and rather than rasing error just set default values
//
impl<'de> Deserialize<'de> for HealthCheck {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize, Debug)]
        #[serde(field_identifier, rename_all = "camelCase")]
        enum Field {
            Id,
            MonitorId,
            ServiceId,
            MemberId,
            PeerId,
            Source,
            Type,
            Status,
            ResponseTimeMs,
            CreatedAt,
            Record,
        }

        struct HealthCheckVisitor;

        impl<'de> Visitor<'de> for HealthCheckVisitor {
            type Value = HealthCheck;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct HealthCheck")
            }

            fn visit_map<V>(self, mut map: V) -> Result<HealthCheck, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut id: Option<u32> = None;
                let mut monitor_id: Option<String> = None;
                let mut service_id: Option<String> = None;
                let mut member_id: Option<String> = None;
                let mut peer_id: Option<String> = None;
                let mut source: Option<Source> = None;
                let mut r#type: Option<Type> = None;
                let mut status: Option<Status> = None;
                let mut response_time_ms: Option<f64> = None;
                let mut created_at: Option<String> = None;
                let mut record: Option<Record> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Id => {
                            if id.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id = Some(map.next_value()?);
                        }
                        Field::MonitorId => {
                            if monitor_id.is_some() {
                                return Err(serde::de::Error::duplicate_field("monitor_id"));
                            }
                            monitor_id = Some(map.next_value()?);
                        }
                        Field::ServiceId => {
                            if service_id.is_some() {
                                return Err(serde::de::Error::duplicate_field("service_id"));
                            }
                            service_id = Some(map.next_value()?);
                        }
                        Field::MemberId => {
                            if member_id.is_some() {
                                return Err(serde::de::Error::duplicate_field("member_id"));
                            }
                            member_id = Some(map.next_value()?);
                        }
                        Field::PeerId => {
                            if peer_id.is_some() {
                                return Err(serde::de::Error::duplicate_field("peer_id"));
                            }
                            peer_id = Some(map.next_value()?);
                        }
                        Field::Source => {
                            if source.is_some() {
                                return Err(serde::de::Error::duplicate_field("source"));
                            }
                            source = Some(map.next_value()?);
                        }
                        Field::Type => {
                            if r#type.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type = Some(map.next_value()?);
                        }
                        Field::Status => {
                            if status.is_some() {
                                return Err(serde::de::Error::duplicate_field("status"));
                            }
                            status = Some(map.next_value()?);
                        }
                        Field::ResponseTimeMs => {
                            if response_time_ms.is_some() {
                                return Err(serde::de::Error::duplicate_field("response_time_ms"));
                            }
                            response_time_ms = Some(map.next_value()?);
                        }
                        Field::CreatedAt => {
                            if created_at.is_some() {
                                return Err(serde::de::Error::duplicate_field("created_at"));
                            }
                            created_at = Some(map.next_value()?);
                        }
                        Field::Record => {
                            if record.is_some() {
                                return Err(serde::de::Error::duplicate_field("record"));
                            }
                            record = Some(map.next_value()?);
                        }
                    }
                }
                let id = id.unwrap_or_default();
                let monitor_id = monitor_id.unwrap_or_default();
                let service_id = service_id.unwrap_or_default();
                let member_id = member_id.unwrap_or_default();
                let peer_id = peer_id.unwrap_or_default();
                let source = source.unwrap_or_default();
                let r#type = r#type.unwrap_or_default();
                let status = status.unwrap_or_default();
                let response_time_ms = response_time_ms.unwrap_or_default();
                let created_at = created_at.unwrap_or_default();
                let record = record.unwrap_or_default();

                Ok(HealthCheck::new(
                    id,
                    monitor_id,
                    service_id,
                    member_id,
                    peer_id,
                    source,
                    r#type,
                    status,
                    response_time_ms,
                    created_at,
                    record,
                ))
            }
        }

        const FIELDS: &'static [&'static str] = &[
            "id",
            "monitor_id",
            "service_id",
            "member_id",
            "peer_id",
            "source",
            "type",
            "status",
            "response_time_ms",
            "created_at",
            "record",
        ];
        deserializer.deserialize_struct("HealthCheck", FIELDS, HealthCheckVisitor)
    }
}

impl HealthCheck {
    pub fn new(
        id: u32,
        monitor_id: String,
        service_id: String,
        member_id: String,
        peer_id: String,
        source: Source,
        r#type: Type,
        status: Status,
        response_time_ms: f64,
        created_at: String,
        record: Record,
    ) -> Self {
        Self {
            id,
            monitor_id,
            service_id,
            member_id,
            peer_id,
            source,
            r#type,
            status,
            response_time_ms,
            created_at,
            record,
        }
    }
}

fn api_health_check_callback(payload: Payload, _socket: RawClient) {
    let config = CONFIG.clone();
    match payload {
        Payload::String(str) => {
            println!("Received: {:#?}", str);
            let hc: HealthCheck = serde_json::from_str(&str).unwrap_or_default();
            println!("HealthCheck: {:#?}", hc)
        }
        Payload::Binary(bin_data) => println!("Received bytes: {:#?}", bin_data),
    }
}

fn api_error_callback(err: Payload, socket: RawClient) {
    let config = CONFIG.clone();
    error!("Monitor server error: {:#?}", err);
    socket.disconnect().expect("Disconnect failed");
    thread::sleep(time::Duration::from_secs(config.error_interval));
    try_to_connect_monitor();
}

// spawns a task to connect and receice a stream of healthchecks
pub fn try_to_connect_monitor() {
    async_std::task::spawn(async {
        let config = CONFIG.clone();
        let url = format!(
            "{}/?apiKey={}",
            config.monitor_api_url, config.monitor_api_key
        );
        info!("Monitor connecting to {}", config.monitor_api_url);
        // get a socket that is connected to the admin namespace
        match ClientBuilder::new(url)
            .transport_type(TransportType::Websocket)
            .on("healthCheck", api_health_check_callback)
            .on("error", api_error_callback)
            .connect()
        {
            Ok(socket) => {
                // TODO: socket.emit("message", "subscribe_healthCheck")
                if let Err(e) = socket.emit("subscribe_healthCheck", "") {
                    error!("Monitor subscription error: {:#?}", e);
                    thread::sleep(time::Duration::from_secs(config.error_interval));
                    try_to_connect_monitor();
                }
            }
            Err(e) => {
                error!("Monitor connection error: {:#?}", e);
                thread::sleep(time::Duration::from_secs(config.error_interval));
                try_to_connect_monitor();
            }
        };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_deserializes_record_struct() {
        let str = "{\"monitorId\":\"12D3KooWK88CwRP1eHSoHheuQbXFcQrQMni2cgVDmB8bu9NtaqVu\",\"memberId\":\"stakeplus\",\"serviceId\":\"statemint-rpc\",\"endpoint\":\"wss://sys.dotters.network/statemint\",\"ipAddress\":\"192.96.202.175\",\"chain\":\"Statemint\",\"chainType\":{\"live\": null},\"health\":{\"peers\":9,\"isSyncing\":false,\"shouldHavePeers\":true},\"syncState\":{\"startingBlock\":4030413,\"currentBlock\":4035043,\"highestBlock\":4035043},\"finalizedBlock\":4035043,\"version\":\"0.9.420-843a5095544\",\"performance\":81.42337107658386}".to_string();
        let a: Record = serde_json::from_str(&str).unwrap_or_default();
        let b = Record {
            member_id: "stakeplus".to_string(),
            monitor_id: "12D3KooWK88CwRP1eHSoHheuQbXFcQrQMni2cgVDmB8bu9NtaqVu".to_string(),
            service_id: "statemint-rpc".to_string(),
            endpoint: "wss://sys.dotters.network/statemint".to_string(),
            ip_address: "192.96.202.175".to_string(),
            chain: "Statemint".to_string(),
            chain_type: ChainType { live: None },
            version: "0.9.420-843a5095544".to_string(),
            finalized_block: 4035043,
            performance: 81.42337107658386,
            ..Default::default()
        };
        assert_eq!(a.member_id, b.member_id);
        assert_eq!(a.monitor_id, b.monitor_id);
        assert_eq!(a.service_id, b.service_id);
        assert_eq!(a.endpoint, b.endpoint);
        assert_eq!(a.ip_address, b.ip_address);
        assert_eq!(a.chain, b.chain);
        assert_eq!(a.chain_type, b.chain_type);
        assert_eq!(a.version, b.version);
        assert_eq!(a.finalized_block, b.finalized_block);
        assert_eq!(a.performance, b.performance);
    }

    #[test]
    fn it_deserializes_health_check_struct() {
        let str = "{\"createdAt\":\"2023-06-27T22:57:09.515Z\",\"id\":31592,\"memberId\":\"turboflakes\",\"monitorId\":\"12D3KooWCyJvRNHQjYLnEVYzR21b9jLKuKLB5LVEijbwxWoqRscP\",\"peerId\":\"12D3KooWQoBwf5FBJBYcgmV3MYu4Fnm47YPe2Ssi5DegViZgcicA\",\"responseTimeMs\":50.19469100236893,\"serviceId\":\"polkadot-rpc\",\"source\":\"check\",\"status\":\"success\",\"type\":\"service_check\",\"record\":{\"monitorId\":\"12D3KooWK88CwRP1eHSoHheuQbXFcQrQMni2cgVDmB8bu9NtaqVu\",\"memberId\":\"stakeplus\",\"serviceId\":\"statemint-rpc\",\"endpoint\":\"wss://sys.dotters.network/statemint\",\"ipAddress\":\"192.96.202.175\",\"chain\":\"Statemint\",\"health\":{\"peers\":9,\"isSyncing\":false,\"shouldHavePeers\":true},\"syncState\":{\"startingBlock\":4030413,\"currentBlock\":4035043,\"highestBlock\":4035043},\"finalizedBlock\":4035043,\"version\":\"0.9.420-843a5095544\",\"performance\":81.42337107658386}}".to_string();
        let a: HealthCheck = serde_json::from_str(&str).unwrap_or_default();
        let b = HealthCheck {
            id: 31592,
            member_id: "turboflakes".to_string(),
            monitor_id: "12D3KooWCyJvRNHQjYLnEVYzR21b9jLKuKLB5LVEijbwxWoqRscP".to_string(),
            peer_id: "12D3KooWQoBwf5FBJBYcgmV3MYu4Fnm47YPe2Ssi5DegViZgcicA".to_string(),
            service_id: "polkadot-rpc".to_string(),
            response_time_ms: 50.19469100236893,
            source: Source::Check,
            status: Status::Success,
            r#type: Type::ServiceCheck,
            ..Default::default()
        };
        assert_eq!(a.id, b.id);
        assert_eq!(a.member_id, b.member_id);
        assert_eq!(a.monitor_id, b.monitor_id);
        assert_eq!(a.peer_id, b.peer_id);
        assert_eq!(a.service_id, b.service_id);
        assert_eq!(a.source, b.source);
        assert_eq!(a.status, b.status);
        assert_eq!(a.r#type, b.r#type);
        assert_eq!(a.response_time_ms, b.response_time_ms);
    }
}
