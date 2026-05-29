// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Matthew Jackson

use axum::http::StatusCode;

pub(crate) enum Verdict {
    Relay,
    RateLimit,
    Transient(&'static str),
    Billing,
    Auth,
}
pub(crate) fn classify(status: StatusCode, body: &str) -> Verdict {
    if body.contains("1113") || body.contains("nsufficient balance") {
        return Verdict::Billing;
    }
    let c = status.as_u16();
    if c == 401 || c == 403 {
        return Verdict::Auth;
    }
    if c == 429
        || body.contains("1302")
        || body.contains("rate_limit")
        || body.contains("Rate limit")
    {
        return Verdict::RateLimit;
    }
    if c >= 500 {
        return Verdict::Transient("5xx");
    }
    Verdict::Relay
}
