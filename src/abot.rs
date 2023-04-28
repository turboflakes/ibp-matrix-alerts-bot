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

use crate::cache::{create_or_await_pool, RedisPool};
use crate::config::{Config, CONFIG};
use crate::errors::{AbotError, CacheError};
use crate::matrix::{Matrix, UserID, MATRIX_SUBSCRIBERS_FILENAME};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::{convert::TryInto, result::Result, thread, time};

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
        // Authenticate matrix and spawn lazy load commands
        spawn_and_restart_matrix_lazy_load_on_error();
    }
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

#[derive(Debug, Deserialize, Clone, PartialEq)]
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

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub enum ReportType {
    Alerts(Option<Severity>),
}

impl ReportType {
    pub fn name(&self) -> String {
        match &self {
            Self::Alerts(severity) => {
                if severity.is_none() {
                    "All Alerts".to_string()
                } else {
                    format!(
                        "Alerts with {} severity",
                        severity.clone().unwrap_or_default()
                    )
                }
            }
        }
    }
}

impl std::fmt::Display for ReportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Alerts(_severity) => write!(f, "Alerts"),
        }
    }
}
