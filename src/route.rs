// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Matthew Jackson

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::forward::forward_with_pool;
use crate::state::{App, WeightedLane};

// POST /<name>/v1/messages   — name resolves to a pool (round-robin) or a single model
pub(crate) async fn named(
    State(app): State<Arc<App>>,
    Path(name): Path<String>,
    body: Bytes,
) -> Response {
    // NOTE: Caller token extraction from request extensions requires handler signature change.
    // For now, caller_token is None - passthrough mode will use lane's api_key as fallback.
    let _caller_token = None;

    if let Some(cands) = app.pools.get(&name) {
        // Convert WeightedLane vec to match forward signature (already same type now)
        return forward_with_pool(app.clone(), cands.clone(), body, _caller_token, &name).await;
    }
    if let Some(&i) = app.by_model.get(&name) {
        // Use forward for model-based routing (no pool name context needed)
        return crate::forward::forward(
            app.clone(),
            vec![WeightedLane { idx: i, weight: 1 }],
            body,
            _caller_token,
        )
        .await;
    }

    (
        StatusCode::NOT_FOUND,
        format!("router: '{name}' is not a known model or pool"),
    )
        .into_response()
}

// POST /<provider>/<model>/v1/messages — ad-hoc direct
pub(crate) async fn adhoc(
    State(app): State<Arc<App>>,
    Path((provider, model)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let _caller_token = None;

    match app.by_model.get(&model) {
        Some(&i) if app.lanes[i].provider == provider => {
            // Single lane with weight=1 (default for ad-hoc routing) - use forward, not forward_with_pool
            crate::forward::forward(
                app.clone(),
                vec![WeightedLane { idx: i, weight: 1 }],
                body,
                _caller_token,
            )
            .await
        }
        Some(&i) => (
            StatusCode::BAD_REQUEST,
            format!(
                "router: model '{}' is on provider '{}', not '{}'",
                model, app.lanes[i].provider, provider
            ),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            format!("router: unknown model '{model}'"),
        )
            .into_response(),
    }
}
