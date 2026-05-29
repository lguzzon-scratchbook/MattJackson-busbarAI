# Busbar

> A fast, native-protocol LLM gateway with weighted pool composition and correct
> billing-vs-client failure handling.

Busbar sits in front of multiple LLM providers and routes each request to a model
or a **pool** of models, with per-member health tracking via a circuit breaker. It
speaks each provider's **native protocol** where it can and only translates when a
request crosses protocols — so homogeneous calls pass through losslessly
(preserving `cache_control`, thinking blocks, citations, native usage accounting).

The name comes from electrical distribution: a busbar takes one feed and fans it
out across many breakered circuits — one entry point, weighted distribution,
per-circuit protection.

> **Project status: early / pre-1.0, in active development.** The shipping path
> today is an Anthropic-format gateway with named/ad-hoc routing, round-robin
> pools, and a circuit breaker. Weighted distribution, OpenAI ingress, and
> cross-protocol failover are in progress. APIs and config may change before 1.0.
> See [`docs/`](docs/) for the design and roadmap.

## Why Busbar

- **Native protocols first.** No mandatory OpenAI-shaped lingua franca; translation
  is confined to cross-protocol hops, so provider-specific features survive.
- **Correct failure semantics.** A backend is ejected for *upstream* faults (5xx,
  overload, rate-limit, billing/quota) but never for *client-supplied* 4xx (a
  malformed or oversized request). A healthy backend is never penalized because a
  caller sent garbage.
- **Single static binary.** Deploy-and-done, no runtime, no GC pauses.

## Quick start

```bash
# Build
cargo build --release

# Configure (copy the example and edit)
cp config.example.yaml config.yaml

# Provider keys come from env vars named in the config (api_key_env)
export ANTHROPIC_KEY=sk-ant-...
export ZAI_KEY=...

# Run
BUSBAR_CONFIG=./config.yaml ./target/release/busbar
```

## API

Clients append `/v1/messages` themselves, matching the Anthropic SDK.

| Method · Route | Purpose |
|---|---|
| `POST /<name>/v1/messages` | `<name>` is a model (`/glm-4.6`) **or** a pool (`/glm`) |
| `POST /<provider>/<model>/v1/messages` | ad-hoc direct route (`/z.ai/glm-4.6`) |
| `GET /stats` | per-member health, counts, and pool membership (JSON) |
| `GET /healthz` | `200` if any member is usable, else `503` |

The router rewrites the request's `model` field and injects the provider's
credential — the caller's own model/key fields are ignored.

## Configuration

See [`config.example.yaml`](config.example.yaml). Providers declare a `base_url`
and the **env var name** holding their key (keys are never stored in the config
file). Models declare a `provider` and `max_concurrent`; pools are named lists of
models whose concurrency caps stack into one aggregate.

## Contributing

Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). To report a
security issue, see [SECURITY.md](SECURITY.md).

## License

Busbar is licensed under the **GNU Affero General Public License v3.0 or later**
([AGPL-3.0-or-later](LICENSE)). Because Busbar is typically run as a network
service, the AGPL's §13 network-use clause applies: if you run a modified Busbar
and let others interact with it over a network, you must offer them the
corresponding modified source.
