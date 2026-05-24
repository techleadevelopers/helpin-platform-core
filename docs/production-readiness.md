# ZooHelp Production Readiness Gate

This is the technical gate before claiming real production readiness for emergency traffic.

The backend is production-shaped, has real observability hooks, and already has durable tables for several critical paths. It is not a full global emergency platform until every item below is verified in staging with evidence.

## 1. Complete Persistence

Required:

- no public critical endpoint may depend on seed data
- posts, rescue sessions, chat rooms, chat messages, notifications, reports, moderation jobs, push subscriptions, audit events and support tickets must be database-backed
- every critical write must have an idempotency key or conflict-safe unique constraint
- every emergency write must return a durable identifier before UI reports success

Evidence:

- migration list
- contract tests
- replay test after API restart
- backup restore test

## 2. Durable Notifications

Required:

- push subscriptions stored in PostgreSQL
- notification event stored before delivery attempt
- push delivery job stored with status
- rescue fanout state stored before phased delivery
- rescue helper responses stored before public UI shows someone going
- provider response persisted
- delivery retry state persisted
- no successful UI state based only on in-memory broadcast

Current production-shaped tables:

- `push_subscriptions`
- `notification_events`
- `push_delivery_jobs`
- `rescue_fanout_states`
- `rescue_fanout_attempts`
- `rescue_responses`
- `rescue_specialist_providers`
- `rescue_escalation_attempts`

Required before scale:

- real Expo/FCM/APNs client
- delivery receipts
- invalid token cleanup
- queue depth alerts

## 3. Queue Guarantees

Required:

- rescue alert/fanout event persisted before fanout
- fanout phase and next run time persisted before worker processing
- workers claim jobs with `FOR UPDATE SKIP LOCKED` or equivalent lease
- duplicate processing must be idempotent
- duplicate push per user/post must be blocked or deduped
- confirmed `Estou indo` must pause aggressive expansion without marking the case resolved
- exhausted local fanout must escalate by competence, not generic broadcast
- queue lag must be measured
- queue consumers must be horizontally safe

Minimum acceptable semantics:

- at-least-once delivery from queue to worker
- idempotent side effects
- durable failure record
- replayable jobs

Do not claim exactly-once. Use at-least-once plus idempotency.

## 4. Retries

Required:

- exponential backoff
- max attempt count
- provider error classification
- retryable vs permanent failure distinction
- persisted `next_attempt_at`
- retry metrics in Prometheus

Current worker behavior:

- claims jobs in database
- increments attempts
- backs off failed jobs
- moves exhausted jobs to `dead_letter`

Still required:

- real push provider implementation
- provider-specific failure mapping
- alert when retry queue age exceeds SLA

## 5. DLQ

Required:

- dead-letter status persisted
- admin surface displays DLQ count
- operator can inspect failed payload and error
- operator can retry or discard after review
- DLQ must be part of incident runbook

Current metric:

- `zoohelp_push_jobs_dead_letter`

Required admin workflow:

- list DLQ jobs
- inspect payload
- retry selected jobs
- mark as acknowledged

## 6. Operational Evidence

Required:

- Grafana dashboard screenshot from staging
- Prometheus metrics scrape from `/metrics`
- OpenTelemetry trace for post -> fanout state -> push job -> notification
- k6/Locust/Vegeta benchmark report
- API restart test proving persistence
- worker restart test proving queued jobs resume

## Release Rule

MVP city pilot can proceed only when:

- emergency post returns only after database commit
- user feed shows post immediately
- rescue fanout state is persisted
- notification event/push job is persisted
- `Estou indo` is persisted as a rescue response before the mobile UI marks `Indo`
- specialist escalation attempts are persisted after local fanout exhaustion
- failed push jobs go to retry/DLQ
- admin observability shows DB, queue and DLQ state
- backup restore is tested

Global production requires the same guarantees plus regional queues, stronger rate limits, provider receipts, human moderation operations and incident runbooks.
