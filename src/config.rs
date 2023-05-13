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

// Load environment variables into a Config struct
//
// Envy is a library for deserializing environment variables into
// typesafe structs
//
// Dotenv loads environment variables from a .env file, if available,
// and mashes those with the actual environment variables provided by
// the operative system.
//
// Set Config struct into a CONFIG lazy_static to avoid multiple processing.
//
use clap::{App, Arg};
use dotenv;
use lazy_static::lazy_static;
use log::info;
use serde::Deserialize;
use std::env;

// Set Config struct into a CONFIG lazy_static to avoid multiple processing
lazy_static! {
    pub static ref CONFIG: Config = get_config();
}

/// provides default value (minutes) for mute_time if ABOT_MUTE_TIME env var is not set
fn default_mute_time() -> u32 {
    5
}

/// provides default value (minutes) for error interval if ABOT_ERROR_INTERVAL env var is not set
fn default_error_interval() -> u64 {
    30
}

/// provides default value for data_path if ABOT_DATA_PATH env var is not set
fn default_data_path() -> String {
    "./".into()
}

/// provides default value for api_host if ONET_API_HOST env var is not set
fn default_api_host() -> String {
    "127.0.0.1".into()
}

/// provides default value for api_port if ONET_API_PORT env var is not set
fn default_api_port() -> u16 {
    5010
}

/// provides default value for api_port if ONET_API_PORT env var is not set
fn default_api_cors_allow_origin() -> String {
    "*".into()
}

/// provides default value for redis_host if ABOT_REDIS_HOST env var is not set
fn default_redis_host() -> String {
    "127.0.0.1".into()
}

/// provides default value for redis_database if ABOT_REDIS_DATABASE env var is not set
fn default_redis_database() -> u8 {
    0
}

#[derive(Clone, Deserialize, Debug)]
pub struct Config {
    // general configuration
    pub api_keys: Vec<String>,
    #[serde(default)]
    pub members_json_url: String,
    #[serde(default)]
    pub ibp_monitor_url: String,
    #[serde(default = "default_mute_time")]
    pub mute_time: u32,
    #[serde(default = "default_error_interval")]
    pub error_interval: u64,
    #[serde(default)]
    pub is_debug: bool,
    #[serde(default = "default_data_path")]
    pub data_path: String,
    // matrix configuration
    #[serde(default)]
    pub matrix_public_room: String,
    #[serde(default)]
    pub matrix_bot_user: String,
    #[serde(default)]
    pub matrix_bot_password: String,
    #[serde(default)]
    pub matrix_disabled: bool,
    #[serde(default)]
    pub matrix_public_room_disabled: bool,
    #[serde(default)]
    pub matrix_bot_display_name_disabled: bool,
    // api
    #[serde(default = "default_api_host")]
    pub api_host: String,
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    #[serde(default = "default_api_cors_allow_origin")]
    pub api_cors_allow_origin: String,
    // redis configuration
    #[serde(default = "default_redis_host")]
    pub redis_hostname: String,
    #[serde(default)]
    pub redis_password: String,
    #[serde(default = "default_redis_database")]
    pub redis_database: u8,
}

/// Inject dotenv and env vars into the Config struct
fn get_config() -> Config {
    // Define CLI flags with clap
    let matches = App::new(env!("CARGO_PKG_NAME"))
    .version(env!("CARGO_PKG_VERSION"))
    .author(env!("CARGO_PKG_AUTHORS"))
    .about(env!("CARGO_PKG_DESCRIPTION"))
    .arg(
        Arg::with_name("api-keys")
          .long("api-keys")
          .takes_value(true)
          .help(
            "API Key to protect api endpoints. If needed specify more than one (e.g. api_key_1,api_key_2,api_key_3).",
          ))
    .arg(
      Arg::with_name("matrix-bot-user")
        .long("matrix-bot-user")
        .takes_value(true)
        .help("Your new 'ABOT' matrix user. e.g. '@matrix-bot-account:matrix.org' this user account will be your 'ABOT' which will be responsible to send messages/notifications to private or public 'IBP-ALERTS' room."))
    .arg(
      Arg::with_name("matrix-bot-password")
        .long("matrix-bot-password")
        .takes_value(true)
        .help("Password for the 'ABOT' matrix user sign in."))
    .arg(
      Arg::with_name("disable-matrix")
        .long("disable-matrix")
        .help(
          "Disable matrix. (e.g. with this flag active, messages/notifications will not be delivered) (https://matrix.org/)",
        ),
    )
    .arg(
      Arg::with_name("disable-matrix-bot-display-name")
        .long("disable-matrix-bot-display-name")
        .help(
          "Disable matrix display name update. (e.g. with this flag active, the default matrix display name user will not be changed.)",
        ),
      )
    .arg(
      Arg::with_name("error-interval")
        .long("error-interval")
        .takes_value(true)
        .default_value("30")
        .help("Interval value (in minutes) from which the bot restarts after a critical error."))
    .arg(
        Arg::with_name("debug")
          .long("debug")
          .help("Prints debug information verbosely."))
    .arg(
      Arg::with_name("config-path")
        .short("c")
        .long("config-path")
        .takes_value(true)
        .value_name("FILE")
        .default_value(".env")
        .help(
          "Sets a custom config file path. The config file contains the bot configuration variables.",
        ),
    )
    .get_matches();

    // Try to load configuration from file first
    let config_path = matches.value_of("config-path").unwrap_or(".env");

    match dotenv::from_filename(&config_path).ok() {
        Some(_) => info!("Loading configuration from {} file", &config_path),
        None => {
            let config_path = env::var("ABOT_CONFIG_FILENAME").unwrap_or(".env".to_string());
            if let Some(_) = dotenv::from_filename(&config_path).ok() {
                info!("Loading configuration from {} file", &config_path);
            }
        }
    }

    if let Some(api_keys) = matches.value_of("api-keys") {
        env::set_var("ABOT_API_KEYS", api_keys);
    }

    if let Some(members_json_url) = matches.value_of("members-json-url") {
        env::set_var("ABOT_MEMBERS_JSON_URL", members_json_url);
    }

    if matches.is_present("debug") {
        env::set_var("ABOT_IS_DEBUG", "true");
    }

    if let Some(data_path) = matches.value_of("data-path") {
        env::set_var("ABOT_DATA_PATH", data_path);
    }

    if matches.is_present("disable-matrix") {
        env::set_var("ABOT_MATRIX_DISABLED", "true");
    }

    if let Some(matrix_bot_user) = matches.value_of("matrix-bot-user") {
        env::set_var("ABOT_MATRIX_BOT_USER", matrix_bot_user);
    }

    if let Some(matrix_bot_password) = matches.value_of("matrix-bot-password") {
        env::set_var("ABOT_MATRIX_BOT_PASSWORD", matrix_bot_password);
    }

    if let Some(error_interval) = matches.value_of("error-interval") {
        env::set_var("ABOT_ERROR_INTERVAL", error_interval);
    }

    match envy::prefixed("ABOT_").from_env::<Config>() {
        Ok(config) => config,
        Err(error) => panic!("Configuration error: {:#?}", error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_gets_a_config() {
        let config = get_config();
        assert_ne!(config.data_path, "".to_string());
    }

    #[test]
    fn it_gets_a_config_from_the_lazy_static() {
        let config = &CONFIG;
        assert_ne!(config.data_path, "".to_string());
    }
}
