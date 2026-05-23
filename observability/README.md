# ZooHelp Evidence-Driven Observability

This folder contains the local/staging observability stack used to prove system health with real artifacts.

It is intentionally evidence-driven:

- Prometheus scrapes the Rust backend `/metrics`.
- Grafana is provisioned with a ZooHelp dashboard.
- OpenTelemetry Collector receives OTLP traces on `4317` and forwards them to Tempo.
- Tempo stores traces for request-path investigation.
- Screenshots belong in `observability/screenshots/` after a real run. Do not commit fake screenshots.

## Start

```powershell
docker compose up -d postgres redis nats prometheus grafana tempo otel-collector
```

Run the backend with:

```powershell
$env:OTEL_EXPORTER_OTLP_ENDPOINT = "http://localhost:4317"
$env:RUST_LOG = "zoohelp_backend=debug,tower_http=info"
cargo run
```

Open:

- Prometheus: `http://localhost:9090`
- Grafana: `http://localhost:3001`
- Tempo: queried through Grafana datasource
- Backend metrics: `http://localhost:8080/metrics`
- Backend status: `http://localhost:8080/v1/observability`

Default Grafana login:

- user: `admin`
- password: `admin`

## Required Evidence For Recruiting Claims

Before presenting observability as production proof, attach:

- Grafana screenshot of the ZooHelp Core Overview dashboard.
- Prometheus query screenshot for `zoohelp_database_latency_ms`.
- Tempo trace screenshot for a rescue/feed/chat request.
- k6/Locust/Vegeta report from `benchmarks/reports/`.
- Commit SHA and environment description.

Suggested screenshot names:

- `screenshots/grafana-core-overview-YYYY-MM-DD.png`
- `screenshots/prometheus-db-latency-YYYY-MM-DD.png`
- `screenshots/tempo-rescue-trace-YYYY-MM-DD.png`

## Production Notes

For production, do not expose Prometheus, Grafana, Tempo, or the OTEL collector publicly. Put them behind private networking, SSO, TLS, and least-privilege access.
