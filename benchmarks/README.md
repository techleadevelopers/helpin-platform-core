# ZooHelp Benchmark Harness

This folder contains executable load-test assets for the ZooHelp backend.

The goal is evidence, not marketing text. Do not claim "10k concurrent sockets", "global scale", or a specific p95 until a report from the target infrastructure is attached under `benchmarks/reports/`.

## Tools

| Tool | Purpose |
|------|---------|
| k6 | HTTP latency, API regression, WebSocket concurrency |
| Locust | User-behavior load model with weighted routes |
| Vegeta | Fast CLI checks for feed, geo, search, health |

## Environment

Default target:

```powershell
$env:BASE_URL = "http://127.0.0.1:8080"
```

Optional location parameters:

```powershell
$env:LAT = "-23.5505"
$env:LNG = "-46.6333"
$env:RADIUS_KM = "25"
```

Authenticated WebSocket tests require a real chat room and a real access token:

```powershell
$env:ROOM_ID = "00000000-0000-0000-0000-000000000000"
$env:ACCESS_TOKEN = "<jwt>"
```

## k6 HTTP Latency

```powershell
k6 run .\benchmarks\k6\http-rescue-feed.js
```

Larger run:

```powershell
$env:K6_VUS = "500"
$env:K6_DURATION = "10m"
k6 run .\benchmarks\k6\http-rescue-feed.js
```

Targets covered:

- `/healthz`
- `/readyz`
- `/v1/feed`
- `/v1/geo/nearby`
- `/v1/search`
- optional authenticated `/v1/notifications`

## Geo Query Benchmarks

Current cloud-compatible latitude/longitude query:

```powershell
psql "$env:DATABASE_URL" -v lat=-23.5505 -v lng=-46.6333 -v radius_km=25 -f .\benchmarks\sql\geo-query-fallback.sql
```

Future PostGIS query after the cloud plan supports it:

```powershell
psql "$env:DATABASE_URL" -v lat=-23.5505 -v lng=-46.6333 -v radius_m=25000 -f .\benchmarks\sql\geo-query-postgis.sql
```

Store `EXPLAIN (ANALYZE, BUFFERS)` output in the report when claiming geo performance.

## k6 WebSocket Scale

Smoke test:

```powershell
$env:K6_WS_VUS = "100"
$env:K6_DURATION = "2m"
k6 run .\benchmarks\k6\websocket-chat.js
```

Candidate 10k socket run:

```powershell
$env:K6_WS_VUS = "10000"
$env:K6_DURATION = "10m"
$env:WS_SESSION_MS = "60000"
k6 run .\benchmarks\k6\websocket-chat.js
```

For 10k real sockets, run from dedicated load generators. Validate OS file descriptor limits, ephemeral ports, CPU, memory, API replicas, Redis/NATS, database pool, and network bandwidth before treating the result as meaningful.

## Locust User Model

```powershell
locust -f .\benchmarks\locust\locustfile.py --host http://127.0.0.1:8080
```

Headless example:

```powershell
locust -f .\benchmarks\locust\locustfile.py --headless -u 1000 -r 50 -t 10m --host http://127.0.0.1:8080
```

## Vegeta CLI Check

```powershell
vegeta attack -duration=60s -rate=100 -targets=.\benchmarks\vegeta\feed.targets | vegeta report
```

JSON output for CI artifact:

```powershell
vegeta attack -duration=60s -rate=100 -targets=.\benchmarks\vegeta\feed.targets | vegeta encode > .\benchmarks\reports\vegeta-feed.json
```

## Required Report Before Public Claims

Every public performance claim should include:

- git commit SHA
- deployment topology
- machine types and regions
- database size and indexes
- Redis/NATS enabled status
- PostGIS enabled status
- user/socket count
- request rate
- p50/p95/p99 latency
- error rate
- CPU, memory, DB pool, queue depth
- test duration
- raw output location

Use `benchmarks/reports/README.md` as the report template.
