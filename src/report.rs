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

use crate::abot::{MemberId, ServiceId, Severity};
use log::info;

type Body = Vec<String>;

pub struct Report {
    body: Body,
}

impl Report {
    pub fn new() -> Report {
        Report { body: Vec::new() }
    }

    pub fn add_raw_text(&mut self, t: String) {
        self.body.push(t);
    }

    pub fn add_break(&mut self) {
        self.add_raw_text("".into());
    }

    pub fn message(&self) -> String {
        self.body.join("\n")
    }

    pub fn formatted_message(&self) -> String {
        self.body.join("<br>")
    }

    pub fn log(&self) {
        info!("__START__");
        for t in &self.body {
            info!("{}", t);
        }
        info!("__END__");
    }
}

#[derive(Debug, Clone)]
pub struct RawAlert {
    pub code: u32,
    pub severity: Severity,
    pub message: String,
    pub member_id: MemberId,
    pub service_id: ServiceId,
}

impl From<RawAlert> for Report {
    /// Converts an ibp-monitor `Alert` into a [`Report`].
    fn from(data: RawAlert) -> Report {
        let mut report = Report::new();

        report.add_raw_text(format!(
            "🚨 <b>Alert code: {}</b> {}",
            data.code,
            severity_emoji(data.severity)
        ));

        report.add_raw_text(format!("‣ 🦸 {} ({})", data.member_id, data.service_id));

        report.add_raw_text(format!("‣ 💬 {}", data.message,));

        report.add_raw_text("——".into());
        report.add_break();

        // Log report
        report.log();

        report
    }
}

fn severity_emoji(severity: Severity) -> String {
    match severity {
        Severity::High => String::from("🔥🔥🔥"),
        Severity::Medium => String::from("🔥🔥"),
        Severity::Low => String::from("🔥"),
        _ => String::from(""),
    }
}
