# Busbar

**The reliability layer for LLM traffic.** One endpoint speaks every major SDK; fault-aware circuit breaking and in-flight failover keep your app serving when your providers aren't.

[![CI](https://github.com/MattJackson/busbarAI/actions/workflows/ci.yml/badge.svg)](https://github.com/MattJackson/busbarAI/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/MattJackson/busbarAI?include_prereleases)](https://github.com/MattJackson/busbarAI/releases)
[![License: AGPL v3](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
![Status](https://img.shields.io/badge/status-1.0.0--rc.4-blue)
![Rust](https://img.shields.io/badge/built%20with-Rust-orange)

📖 **Docs: [ai-bus.bar](https://ai-bus.bar)** · ⚡ **Install:** `curl -fsSL https://ai-bus.bar/install.sh | sh` · 🤖 **Agent-readable:** [ai-bus.bar/llms.txt](https://ai-bus.bar/llms.txt)

Busbar is a gateway that sits between your application and your LLM providers. Point any SDK — OpenAI, Anthropic, Gemini, Bedrock, Cohere — at one URL, and it routes, translates, and **keeps serving through provider failures**.

> **You define a model name and its backends. Busbar accepts _any_ input protocol — OpenAI, Anthropic, Gemini, Bedrock, Cohere, Responses — and routes and translates accordingly.** One model name, reachable by every client; you choose what runs behind it.

Three things make it a different class of tool than a proxy with a long model list:

**1. It speaks every protocol losslessly — both ways.**
Six wire protocols, native on ingress *and* egress, translated through one internal format rich enough to hold every protocol's features — so nothing gets dropped. Busbar does **not** flatten everything to OpenAI shape the way most gateways do, so provider-native features — Anthropic thinking blocks, Gemini safety settings, Bedrock tool use — survive the hop.
→ *What that enables:* use whatever SDK your code already speaks — and reach **every** model through it; move a workload from Claude to Gemini with a config edit instead of a code migration; adopt a new model the day it ships.

**2. It fails over inside the request — before your client sees a byte, even mid-stream.**
→ *What that enables:* a provider 429 or 5xx becomes a silent reroute across your pool — including **across protocol families**, Anthropic → OpenAI → Gemini — not a 500 your user feels and not a 3am page. Reliability your app gets for free, instead of a pile of per-provider retry code.

**3. It knows whose fault a failure is.**
A circuit breaker on every provider connection classifies every error — provider outage, *your* bad request, context-length, hard auth/billing failure — and treats each differently instead of retrying into a wall.
→ *What that enables:* a flaky provider is pulled from rotation automatically and probed back in gently; a malformed 400 never poisons a healthy lane; an overly long prompt fails *over* to a bigger-context model instead of just failing.

Runs in your own infrastructure today — a single static binary for Linux, macOS, and Windows (Intel and ARM): your keys, your network, your data path, no third party in the middle.

> **Status: 1.0.0-rc.4 — feature-complete and API-stable, hardened across a multi-round security and correctness audit. Release-candidate validation continuing ahead of 1.0.0.** AGPL-3.0.

---

## The one-line change

Your code already speaks OpenAI (or Anthropic, or Gemini). Swap the base URL:

```diff
- client = OpenAI(api_key=OPENAI_KEY)
+ client = OpenAI(api_key=BUSBAR_TOKEN, base_url="http://busbar:8080")

  # the rest of your code is untouched — `model` now names a single model
  # OR a pool you define in config (e.g. "fast" = 80% Claude / 20% GPT-4o, Gemini on failover)
  client.chat.completions.create(
      model="fast",
      messages=[{"role": "user", "content": "Hello!"}],
  )
```

That request left as OpenAI, may have been served by Anthropic, and came back as OpenAI — translated losslessly both ways. If Anthropic returned a 429 mid-flight, Busbar rerouted to the next pool member before your client saw a single byte. The model name is a config value, not a code dependency.

---

## A different class of product

Most LLM gateways are a proxy with a model list: normalize every request to one shape, forward it, list a lot of providers. Busbar is built as **reliability infrastructure** first — the breaker, the in-flight failover, and the lossless translation are the core, not add-ons.

| | Busbar | Self-hosted proxy | Hosted router |
|---|---|---|---|
| **Cross-protocol translation** | Native, lossless both ways — keeps provider-native features | Normalized to OpenAI shape (lossy) | OpenAI shape only |
| **Circuit breaking** | Per-(pool, lane), fault-attributed | Basic retry / cooldown | Not exposed |
| **Failover** | Mid-request, before first byte — streaming-safe, across protocols | Exception-level retry | None |
| **Weighted pools** | Smooth weighted round-robin + session affinity | Limited | Limited |
| **Governance** | Built-in virtual keys, budgets, ACLs | Add-on | Dashboard |
| **Keys & prompts** | Stay in your network | Stay in your network | Transit a third party |
| **Runtime** | Single static binary | Python + dependencies | n/a (hosted) |

If you've used **LiteLLM** or **OpenRouter**: same arena. The difference is depth — fault-attributed circuit breaking, true in-flight (streaming-safe) failover, and *lossless* cross-protocol translation that doesn't make you trade away each provider's native features. Self-hosted is how it ships today, not what it is.

→ The full case, with an honest feature-by-feature comparison: **[Why Busbar](https://ai-bus.bar/why-busbar/)**

---

## 60-second quickstart

**1. Install** (macOS / Linux) — drops `busbar` + `providers.yaml` into the current directory:

```bash
curl -fsSL https://ai-bus.bar/install.sh | sh
```

Or build from source (`cargo build --release`), or grab a binary from the [releases page](https://github.com/MattJackson/busbarAI/releases/latest).

**2. Write a minimal `config.yaml`** — keys live in env vars, named here (never written into config):

```yaml
listen: "0.0.0.0:8080"
auth: { mode: token, client_tokens: ["${BUSBAR_CLIENT_TOKEN}"] }

providers:
  anthropic: { api_key_env: ANTHROPIC_KEY }   # api_key_env = the NAME of the env var, not the key
  openai:    { api_key_env: OPENAI_KEY }

models:
  claude-sonnet: { provider: anthropic, max_concurrent: 20 }
  gpt-4o-mini:   { provider: openai,    max_concurrent: 50 }

pools:
  fast:
    members:
      - { target: claude-sonnet, weight: 8 }
      - { target: gpt-4o-mini,   weight: 2 }
    on_exhausted: { action: least_bad }
```

**3. Run it:**

```bash
export BUSBAR_CLIENT_TOKEN=changeme ANTHROPIC_KEY=sk-ant-... OPENAI_KEY=sk-...
BUSBAR_PROVIDERS=./providers.yaml BUSBAR_CONFIG=./config.yaml ./busbar
```

**4. Send a request** — OpenAI SDK or curl against the `fast` pool; you get a native OpenAI response even if Anthropic served it:

```bash
curl -s http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer $BUSBAR_CLIENT_TOKEN" -H "content-type: application/json" \
  -d '{"model":"fast","messages":[{"role":"user","content":"Hello!"}]}'
```

Full walkthrough: **[Getting Started →](https://ai-bus.bar/getting-started/)**

---

## What's inside

- **Six wire protocols, lossless both ways** — OpenAI, Anthropic, Gemini, Bedrock, Cohere, Responses, native on ingress *and* egress. Any client protocol can reach any pool. → [Protocols](https://ai-bus.bar/protocols/)
- **Fault-attributed circuit breaking** — per-(pool, lane) breakers that tell a provider outage from *your* bad request from a context-length overflow, and act differently on each. → [Reliability](https://ai-bus.bar/reliability/)
- **In-flight, streaming-safe failover** — reroutes mid-request before the client sees a byte, across protocol families. → [Reliability](https://ai-bus.bar/reliability/)
- **Weighted pools** — smooth weighted round-robin, session affinity, per-lane concurrency limits, configurable exhaustion policies. → [Reliability](https://ai-bus.bar/reliability/)
- **Governance** — virtual keys with budgets, RPM/TPM limits, spend tracking, pool access control; embedded SQLite, off by default. → [Governance](https://ai-bus.bar/guides/governance/)
- **A curated, vetted provider catalog** — plus any provider speaking one of the six protocols in a few lines of YAML *you* own. → [Adding a provider](https://ai-bus.bar/providers/)
- **Security-hardened request path** — SSRF guards, constant-time auth, SHA-256 key storage, 32 MiB body cap, native-protocol error envelopes, secrets never logged. → [SECURITY.md](SECURITY.md)
- **Observability built in** — Prometheus `/metrics`, optional OTLP traces, per-request webhook. → [Configuration](https://ai-bus.bar/configuration/)

---

## For managers: why Busbar

If you use more than one LLM provider — or plan to — you are building vendor lock-in into your application layer every day you don't have a gateway. When a provider has an outage, you get paged. When you want to try a new model, you write code. When you need to control spend per team or project, you have no surface to do it on.

Busbar addresses these as infrastructure, not application logic:

- **One vendor's outage stops being your outage** — the breaker detects degradation and failover happens inside a single request, before the client sees a byte.
- **Switching or splitting traffic between models is a config edit, not a deploy** — your app sends `model: "fast"`; you decide what "fast" means in YAML.
- **You keep control of cost** — virtual keys with budgets and rate limits, without building a governance layer.
- **Nothing leaves your network** — Busbar runs in your infra with your keys; prompts don't transit a third party.
- **No runtime tax** — a single ~8 MB static binary. No Python sidecar, no interpreter, no GC in the request path. Deploy it like nginx.

The full decision-maker case: **[Why Busbar →](https://ai-bus.bar/why-busbar/)**

---

## Documentation

Everything lives at **[ai-bus.bar](https://ai-bus.bar)** (agent-readable at [ai-bus.bar/llms.txt](https://ai-bus.bar/llms.txt)):

[Getting Started](https://ai-bus.bar/getting-started/) · [Why Busbar](https://ai-bus.bar/why-busbar/) · [Protocols](https://ai-bus.bar/protocols/) · [Reliability](https://ai-bus.bar/reliability/) · [Governance](https://ai-bus.bar/guides/governance/) · [Adding a Provider](https://ai-bus.bar/providers/) · [Configuration](https://ai-bus.bar/configuration/)

Contributor-facing docs (architecture, internals, ADRs) live in [`docs/`](docs/).

---

## Build and platforms

Single Rust binary, MSRV 1.87, edition 2021. CI builds and tests on Linux and Windows; releases cross-build macOS. Releases ship `x86_64`/`aarch64` Linux, Intel/Apple-Silicon macOS, and `x86_64` Windows.

```bash
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
```

---

## Contributing and license

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).

Licensed **AGPL-3.0-or-later** ([LICENSE](LICENSE)). Because Busbar typically runs as a network service, the AGPL's §13 network-use clause applies: run a modified Busbar and let others reach it over a network, and you must offer them the corresponding modified source.
