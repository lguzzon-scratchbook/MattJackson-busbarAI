// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Matthew Jackson

use std::sync::Arc;

use axum::{body::Body, http::StatusCode, response::IntoResponse, response::Response};
use serde_json::{json, Value};
use tokio::sync::OwnedSemaphorePermit;

use crate::breaker::{classify, Verdict};
use crate::state::{now, App};

async fn pick_among(app: &Arc<App>, cands: &[usize]) -> Option<(usize, OwnedSemaphorePermit)> {
    let t = now();
    let usable: Vec<usize> = cands
        .iter()
        .copied()
        .filter(|&i| app.lanes[i].usable(t))
        .collect();
    if usable.is_empty() {
        return None;
    }
    let start = app.rr.fetch_add(1, Ordering::Relaxed);
    let order: Vec<usize> = (0..usable.len())
        .map(|k| usable[(start + k) % usable.len()])
        .collect();
    for &i in &order {
        if let Ok(p) = app.lanes[i].sem.clone().try_acquire_owned() {
            return Some((i, p));
        }
    }
    let futs: Vec<_> = order
        .iter()
        .map(|&i| {
            let sem = app.lanes[i].sem.clone();
            Box::pin(async move { (i, sem.acquire_owned().await.unwrap()) })
        })
        .collect();
    let ((i, p), _, _) = futures::future::select_all(futs).await;
    Some((i, p))
}

use axum::{body::Bytes, http::header::CONTENT_TYPE};
use std::sync::atomic::Ordering;

pub(crate) async fn forward(app: Arc<App>, cands: Vec<usize>, body: Bytes) -> Response {
    let mut v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("router: bad json: {e}")).into_response()
        }
    };
    let attempts = cands.len() + 2;
    for _ in 0..attempts {
        let (i, permit) = match pick_among(&app, &cands).await {
            Some(x) => x,
            None => {
                return (StatusCode::SERVICE_UNAVAILABLE, "router: no usable lane").into_response()
            }
        };
        v["model"] = json!(app.lanes[i].model);
        let payload = serde_json::to_vec(&v).unwrap();
        let (base, key) = (app.lanes[i].base_url.clone(), app.lanes[i].api_key.clone());
        app.lanes[i].inflight.fetch_add(1, Ordering::Relaxed);
        let res = app
            .client
            .post(format!("{base}/v1/messages"))
            .header("x-api-key", &key)
            .header("authorization", format!("Bearer {key}"))
            .header("anthropic-version", "2023-06-01")
            .header(CONTENT_TYPE, "application/json")
            .body(payload)
            .send()
            .await;
        app.lanes[i].inflight.fetch_sub(1, Ordering::Relaxed);
        match res {
            Err(e) => {
                app.lanes[i].cooldown_transient(if e.is_timeout() { "timeout" } else { "connect" });
                drop(permit);
                continue;
            }
            Ok(r) => {
                let status = r.status();
                let ct = r.headers().get(CONTENT_TYPE).cloned();
                let bytes = r.bytes().await.unwrap_or_default();
                let text = String::from_utf8_lossy(&bytes);
                match classify(status, &text) {
                    Verdict::Billing => {
                        app.lanes[i].kill("billing / insufficient balance (1113)");
                        drop(permit);
                        continue;
                    }
                    Verdict::Auth => {
                        app.lanes[i].kill(&format!("auth rejected (HTTP {})", status.as_u16()));
                        drop(permit);
                        continue;
                    }
                    Verdict::RateLimit => {
                        app.lanes[i].cooldown_rate_limit();
                        drop(permit);
                        continue;
                    }
                    Verdict::Transient(w) => {
                        app.lanes[i].cooldown_transient(w);
                        drop(permit);
                        continue;
                    }
                    Verdict::Relay => {
                        if status.is_success() {
                            app.lanes[i].success();
                        }
                        let mut rb = Response::builder().status(status);
                        if let Some(ct) = ct {
                            rb = rb.header(CONTENT_TYPE, ct);
                        }
                        return rb.body(Body::from(bytes)).unwrap();
                    }
                }
            }
        }
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "router: all lanes exhausted",
    )
        .into_response()
}
