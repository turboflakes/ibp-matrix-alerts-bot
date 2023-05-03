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

mod abot;
mod api;
mod cache;
mod config;
mod errors;
mod matrix;

use crate::abot::Abot;
use crate::api::routes::routes;
use crate::config::CONFIG;
use log::info;
use std::env;

// use actix::*;
use actix_cors::Cors;
use actix_web::{http, middleware, web, App, HttpServer};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // load configuration
    let config = CONFIG.clone();

    if config.is_debug {
        env::set_var("RUST_LOG", "abot=debug");
    } else {
        env::set_var("RUST_LOG", "abot=info");
    }
    env_logger::try_init().unwrap_or_default();

    info!(
        "{} v{} * {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_DESCRIPTION")
    );

    // authenticate matrix user, load and process commands from matrix rooms
    Abot::start();

    // create a new instance to be shared with all webhooks
    let abot = Abot::new().await;

    // start http webhooks server
    let addr = format!("{}:{}", config.api_host, config.api_port);
    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin_fn(|origin, _req_head| {
                let allowed_origins =
                    env::var("ABOT_API_CORS_ALLOW_ORIGIN").unwrap_or("*".to_string());
                let allowed_origins = allowed_origins.split(",").collect::<Vec<_>>();
                allowed_origins
                    .iter()
                    .any(|e| e.as_bytes() == origin.as_bytes())
            })
            .allowed_methods(vec!["GET", "POST", "OPTIONS"])
            .allowed_headers(vec![http::header::CONTENT_TYPE])
            .supports_credentials()
            .max_age(3600);
        App::new()
            .app_data(web::Data::new(abot.clone()))
            .wrap(middleware::Logger::default())
            .wrap(cors)
            .configure(routes)
    })
    .bind(addr)?
    .run()
    .await
}
