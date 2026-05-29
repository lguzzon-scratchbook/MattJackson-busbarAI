// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Matthew Jackson

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct ProviderCfg {
    pub(crate) base_url: String,
    pub(crate) api_key_env: String,
    #[serde(default = "default_protocol")]
    pub(crate) protocol: String,
}

fn default_protocol() -> String {
    "anthropic".to_string()
}
#[derive(Deserialize)]
pub(crate) struct ModelCfg {
    pub(crate) provider: String,
    pub(crate) max_concurrent: usize,
    #[serde(default = "neg1")]
    pub(crate) max_requests: i64,
}
fn neg1() -> i64 {
    -1
}
#[derive(Deserialize)]
pub(crate) struct Cfg {
    #[serde(default = "default_listen")]
    pub(crate) listen: String,
    pub(crate) providers: HashMap<String, ProviderCfg>,
    pub(crate) models: HashMap<String, ModelCfg>,
    #[serde(default)]
    pub(crate) pools: HashMap<String, Vec<String>>,
}
fn default_listen() -> String {
    "0.0.0.0:8080".into()
}
