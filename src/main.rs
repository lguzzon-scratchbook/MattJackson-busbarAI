// busbar — central LLM gateway: round-robin across lanes like a busbar across circuits.
// a PATH that names what they want. Anthropic `/v1/messages` format (v1).
//
// Clean user API (clients append /v1/messages themselves, per the Anthropic SDK):
//   POST /<name>                  name = a model OR a config-defined pool
//        /glm-4.5                 -> that single model
//        /glm                     -> pool: round-robin glm-5.1+glm-4.6+glm-4.5
//        /haiku                   -> claude-haiku via anthropic
//   POST /<provider>/<model>      ad-hoc: any provider+model, no pool needed
//        /z.ai/glm-4.6
//   GET  /stats  /healthz
//
// A round-robin pool stacks its models' per-model concurrency caps into one
// aggregate (10+10+3 = 23). Each model is a "lane" with its own semaphore +
// smart health handling. The caller's own model/key fields are ignored — the
// router rewrites `model` and injects the provider's key. No per-client keys.
//
// Smart lane health:
//   2xx                          -> relay, reset streak
//   billing (z.ai 1113)          -> STOP lane permanently (empty wallet won't heal)
//   auth (401/403)               -> STOP lane permanently (bad key/config)
//   rate limit (429 / z.ai 1302) -> escalating cooldown (15s*streak, cap 120s)
//   5xx / network / timeout      -> short cooldown (10s)
//   other 4xx (400/404/422)      -> RELAY to caller, do NOT penalize the lane
//   max_requests budget hit      -> disable lane (cost cap)
//
// v2: OpenAI-protocol providers (P4/A5500 /v1/chat/completions) need Anthropic
// <-> OpenAI translation; not handled here. All v1 lanes are Anthropic-format.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const COOLDOWN_BASE_SECS: u64 = 15;
const COOLDOWN_MAX_SECS: u64 = 120;
const COOLDOWN_TRANSIENT_SECS: u64 = 10;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

// ---------- config ----------
#[derive(Deserialize)]
struct ProviderCfg {
    base_url: String,
    api_key_env: String,
}
#[derive(Deserialize)]
struct ModelCfg {
    provider: String,
    max_concurrent: usize,
    #[serde(default = "neg1")]
    max_requests: i64,
}
fn neg1() -> i64 {
    -1
}
#[derive(Deserialize)]
struct Cfg {
    #[serde(default = "default_listen")]
    listen: String,
    providers: HashMap<String, ProviderCfg>,
    models: HashMap<String, ModelCfg>,
    #[serde(default)]
    pools: HashMap<String, Vec<String>>,
}
fn default_listen() -> String {
    "0.0.0.0:8080".into()
}

// ---------- lane (one per model) ----------
struct Lane {
    model: String,
    provider: String,
    base_url: String,
    api_key: String,
    sem: Arc<Semaphore>,
    max: usize,
    limited: bool,
    budget: AtomicI64,
    cooldown_until: AtomicU64,
    streak: AtomicU32,
    dead: AtomicBool,
    dead_reason: std::sync::Mutex<String>,
    inflight: AtomicI64,
    ok: AtomicU64,
    err: AtomicU64,
}
impl Lane {
    fn usable(&self, t: u64) -> bool {
        if self.dead.load(Ordering::Relaxed) {
            return false;
        }
        if self.limited && self.budget.load(Ordering::Relaxed) <= 0 {
            return false;
        }
        t >= self.cooldown_until.load(Ordering::Relaxed)
    }
    fn kill(&self, reason: &str) {
        self.dead.store(true, Ordering::Relaxed);
        *self.dead_reason.lock().unwrap() = reason.to_string();
        eprintln!("[{}] STOPPED permanently: {}", self.model, reason);
    }
    fn cooldown_rate_limit(&self) {
        let s = self.streak.fetch_add(1, Ordering::Relaxed) + 1;
        let secs = (COOLDOWN_BASE_SECS * s as u64).min(COOLDOWN_MAX_SECS);
        self.cooldown_until.store(now() + secs, Ordering::Relaxed);
        self.err.fetch_add(1, Ordering::Relaxed);
        eprintln!("[{}] rate-limited (streak {}), cooldown {}s", self.model, s, secs);
    }
    fn cooldown_transient(&self, what: &str) {
        self.cooldown_until.store(now() + COOLDOWN_TRANSIENT_SECS, Ordering::Relaxed);
        self.err.fetch_add(1, Ordering::Relaxed);
        eprintln!("[{}] transient ({}), cooldown {}s", self.model, what, COOLDOWN_TRANSIENT_SECS);
    }
    fn success(&self) {
        self.streak.store(0, Ordering::Relaxed);
        self.ok.fetch_add(1, Ordering::Relaxed);
        if self.limited && self.budget.fetch_sub(1, Ordering::Relaxed) - 1 <= 0 {
            self.kill("request budget exhausted");
        }
    }
}

struct App {
    lanes: Vec<Lane>,
    by_model: HashMap<String, usize>,
    pools: HashMap<String, Vec<usize>>,
    rr: AtomicUsize,
    client: reqwest::Client,
}

async fn pick_among(app: &Arc<App>, cands: &[usize]) -> Option<(usize, OwnedSemaphorePermit)> {
    let t = now();
    let usable: Vec<usize> = cands.iter().copied().filter(|&i| app.lanes[i].usable(t)).collect();
    if usable.is_empty() {
        return None;
    }
    let start = app.rr.fetch_add(1, Ordering::Relaxed);
    let order: Vec<usize> = (0..usable.len()).map(|k| usable[(start + k) % usable.len()]).collect();
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

enum Verdict {
    Relay,
    RateLimit,
    Transient(&'static str),
    Billing,
    Auth,
}
fn classify(status: StatusCode, body: &str) -> Verdict {
    if body.contains("1113") || body.contains("nsufficient balance") {
        return Verdict::Billing;
    }
    let c = status.as_u16();
    if c == 401 || c == 403 {
        return Verdict::Auth;
    }
    if c == 429 || body.contains("1302") || body.contains("rate_limit") || body.contains("Rate limit") {
        return Verdict::RateLimit;
    }
    if c >= 500 {
        return Verdict::Transient("5xx");
    }
    Verdict::Relay
}

async fn forward(app: Arc<App>, cands: Vec<usize>, body: Bytes) -> Response {
    let mut v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("router: bad json: {e}")).into_response(),
    };
    let attempts = cands.len() + 2;
    for _ in 0..attempts {
        let (i, permit) = match pick_among(&app, &cands).await {
            Some(x) => x,
            None => return (StatusCode::SERVICE_UNAVAILABLE, "router: no usable lane").into_response(),
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
                    Verdict::Billing => { app.lanes[i].kill("billing / insufficient balance (1113)"); drop(permit); continue; }
                    Verdict::Auth => { app.lanes[i].kill(&format!("auth rejected (HTTP {})", status.as_u16())); drop(permit); continue; }
                    Verdict::RateLimit => { app.lanes[i].cooldown_rate_limit(); drop(permit); continue; }
                    Verdict::Transient(w) => { app.lanes[i].cooldown_transient(w); drop(permit); continue; }
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
    (StatusCode::SERVICE_UNAVAILABLE, "router: all lanes exhausted").into_response()
}

// POST /<name>/v1/messages   — name resolves to a pool (round-robin) or a single model
async fn named(State(app): State<Arc<App>>, Path(name): Path<String>, body: Bytes) -> Response {
    if let Some(cands) = app.pools.get(&name) {
        return forward(app.clone(), cands.clone(), body).await;
    }
    if let Some(&i) = app.by_model.get(&name) {
        return forward(app.clone(), vec![i], body).await;
    }
    (StatusCode::NOT_FOUND, format!("router: '{name}' is not a known model or pool")).into_response()
}

// POST /<provider>/<model>/v1/messages — ad-hoc direct
async fn adhoc(State(app): State<Arc<App>>, Path((provider, model)): Path<(String, String)>, body: Bytes) -> Response {
    match app.by_model.get(&model) {
        Some(&i) if app.lanes[i].provider == provider => forward(app.clone(), vec![i], body).await,
        Some(&i) => (StatusCode::BAD_REQUEST, format!("router: model '{}' is on provider '{}', not '{}'", model, app.lanes[i].provider, provider)).into_response(),
        None => (StatusCode::NOT_FOUND, format!("router: unknown model '{model}'")).into_response(),
    }
}

async fn stats(State(app): State<Arc<App>>) -> Response {
    let t = now();
    let lanes: Vec<Value> = app.lanes.iter().map(|l| json!({
        "model": l.model, "provider": l.provider, "max_concurrent": l.max,
        "inflight": l.inflight.load(Ordering::Relaxed), "free_slots": l.sem.available_permits(),
        "ok": l.ok.load(Ordering::Relaxed), "err": l.err.load(Ordering::Relaxed),
        "usable": l.usable(t), "dead": l.dead.load(Ordering::Relaxed),
        "dead_reason": *l.dead_reason.lock().unwrap(),
        "cooldown_remaining_s": l.cooldown_until.load(Ordering::Relaxed).saturating_sub(t),
        "streak": l.streak.load(Ordering::Relaxed),
        "budget": if l.limited { l.budget.load(Ordering::Relaxed) } else { -1 },
    })).collect();
    let pools: HashMap<&String, Vec<&str>> = app.pools.iter()
        .map(|(n, idx)| (n, idx.iter().map(|&i| app.lanes[i].model.as_str()).collect())).collect();
    Json(json!({ "pools": pools, "lanes": lanes })).into_response()
}

async fn healthz(State(app): State<Arc<App>>) -> Response {
    let t = now();
    if app.lanes.iter().any(|l| l.usable(t)) {
        (StatusCode::OK, "ok").into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "no usable lanes").into_response()
    }
}

#[tokio::main]
async fn main() {
    let path = std::env::var("BUSBAR_CONFIG").unwrap_or_else(|_| "/etc/busbar/config.json".into());
    let cfg: Cfg = serde_json::from_str(&std::fs::read_to_string(&path).expect("read BUSBAR_CONFIG"))
        .expect("parse config");

    let mut lanes = Vec::new();
    let mut by_model = HashMap::new();
    for (model, mc) in cfg.models {
        let p = cfg.providers.get(&mc.provider).unwrap_or_else(|| panic!("model {model} -> unknown provider {}", mc.provider));
        let key = std::env::var(&p.api_key_env).unwrap_or_default();
        if key.is_empty() {
            eprintln!("[warn] provider {} key env {} empty", mc.provider, p.api_key_env);
        }
        let limited = mc.max_requests >= 0;
        by_model.insert(model.clone(), lanes.len());
        lanes.push(Lane {
            model,
            provider: mc.provider,
            base_url: p.base_url.trim_end_matches('/').to_string(),
            api_key: key,
            sem: Arc::new(Semaphore::new(mc.max_concurrent)),
            max: mc.max_concurrent,
            limited,
            budget: AtomicI64::new(if limited { mc.max_requests } else { -1 }),
            cooldown_until: AtomicU64::new(0),
            streak: AtomicU32::new(0),
            dead: AtomicBool::new(false),
            dead_reason: std::sync::Mutex::new(String::new()),
            inflight: AtomicI64::new(0),
            ok: AtomicU64::new(0),
            err: AtomicU64::new(0),
        });
    }

    let mut pools = HashMap::new();
    for (name, members) in cfg.pools {
        let idx: Vec<usize> = members.iter().map(|m| {
            *by_model.get(m).unwrap_or_else(|| panic!("pool {name} references unknown model {m}"))
        }).collect();
        pools.insert(name, idx);
    }

    eprintln!("busbar: {} models, {} pools", lanes.len(), pools.len());
    for l in &lanes {
        eprintln!("  model {} via {} ({}) max {}", l.model, l.provider, l.base_url, l.max);
    }
    for (n, idx) in &pools {
        let agg: usize = idx.iter().map(|&i| lanes[i].max).sum();
        eprintln!("  pool /{} = [{}] aggregate {}", n, idx.iter().map(|&i| lanes[i].model.clone()).collect::<Vec<_>>().join(", "), agg);
    }

    let app = Arc::new(App {
        lanes,
        by_model,
        pools,
        rr: AtomicUsize::new(0),
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .pool_max_idle_per_host(64)
            .build()
            .unwrap(),
    });

    let listen = cfg.listen.clone();
    let router = Router::new()
        .route("/stats", get(stats))
        .route("/healthz", get(healthz))
        .route("/:name/v1/messages", post(named))
        .route("/:provider/:model/v1/messages", post(adhoc))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(&listen).await.expect("bind");
    eprintln!("llm-router listening on {listen}");
    axum::serve(listener, router).await.unwrap();
}
