// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Matthew Jackson
//
//! The single seam where the JSON library is named.
//!
//! Every hot request/response body parse and serialize on the translate path goes through here, so
//! the implementation (today: sonic-rs, SIMD on the large string-heavy bodies LLM traffic carries)
//! lives in ONE place instead of being scattered as `sonic_rs::`/`serde_json::` across the request
//! path. Swapping the parser/serializer — or, later, eliminating the `serde_json::Value` intermediate
//! in favour of parsing straight into the IR — becomes a change to this module, not a hunt across
//! `route.rs` and `forward.rs`. Cold paths (config load, error envelopes, tests) keep using
//! `serde_json` directly; this seam is for the per-request body hot path only.
//!
//! The in-memory document type remains `serde_json::Value` for now (sonic-rs parses/serializes it
//! directly); the next step toward breaking the proto layer's coupling to a concrete JSON type is to
//! re-export a canonical `Value` here and migrate call sites — a change localized to this module.

/// Parse request/response body bytes into a document. SIMD-accelerated.
#[inline]
pub(crate) fn parse<'de, T: serde::Deserialize<'de>>(
    bytes: &'de [u8],
) -> Result<T, sonic_rs::Error> {
    sonic_rs::from_slice(bytes)
}

/// Serialize a document to body bytes. SIMD-accelerated; the request/response hot-path serializer.
#[inline]
pub(crate) fn to_vec<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, sonic_rs::Error> {
    sonic_rs::to_vec(value)
}
