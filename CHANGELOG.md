# Changelog

All notable changes to Busbar are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Project scaffolding for open-source release: `README`, `CONTRIBUTING`,
  `SECURITY`, issue/PR templates, and CI workflow.

### Changed
- Licensed the project under **AGPL-3.0-or-later** (previously MIT) — the AGPL's
  network-use clause is the appropriate copyleft for a gateway run as a service.

### Notes
- Pre-1.0: the current binary is an Anthropic-format gateway with named/ad-hoc
  routing, round-robin pools, and a circuit breaker. See the roadmap for the path
  to native multi-protocol support, weighted distribution, and cross-protocol
  failover.

[Unreleased]: https://github.com/MattJackson/busbarAI/commits/main
