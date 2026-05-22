# Benchmark Report Template

Create one report file per relevant run, for example:

- `2026-05-22-staging-http-k6.md`
- `2026-05-22-staging-ws-10k-k6.md`
- `2026-05-22-geo-query-vegeta.md`

## Summary

| Field | Value |
|-------|-------|
| Date | |
| Git SHA | |
| Environment | local / staging / production shadow |
| Region | |
| Test owner | |
| Tool | k6 / Locust / Vegeta |
| Duration | |

## Infrastructure

| Component | Value |
|-----------|-------|
| API replicas | |
| API CPU/memory | |
| PostgreSQL version | |
| PostgreSQL size | |
| Redis enabled | yes / no |
| NATS enabled | yes / no |
| PostGIS enabled | yes / no |
| PgBouncer enabled | yes / no |
| Load generator count | |

## Workload

| Metric | Value |
|--------|-------|
| Virtual users | |
| Concurrent WebSockets | |
| Request rate | |
| Routes covered | |
| Seeded posts | |
| Seeded users | |
| Seeded chat rooms | |

## Results

| Metric | Value |
|--------|-------|
| p50 latency | |
| p95 latency | |
| p99 latency | |
| Max latency | |
| Error rate | |
| WebSocket connect p95 | |
| Messages received | |
| DB pool saturation | |
| Redis latency | |
| Queue depth | |

## Bottlenecks Found

- 

## Actions

- 

## Raw Artifacts

- k6 output:
- Locust CSV:
- Vegeta JSON:
- Grafana dashboard:
