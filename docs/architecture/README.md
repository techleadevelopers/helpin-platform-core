# ZooHelp Visual Architecture

These diagrams are source-controlled architecture assets for reviewers, contributors, and recruiting conversations.

They describe the intended production shape while keeping current constraints explicit: PostGIS can be enabled later, Redis/NATS fanout is the scale target, and public performance claims require benchmark reports.

## C4 Context

```mermaid
flowchart LR
    user[Mobile user<br/>Reports animals, requests rescue, chats]
    volunteer[Volunteer / rescuer<br/>Receives nearby alerts and coordinates help]
    ong[NGO / vet partner<br/>Profiles, cases, support and rescue ops]
    admin[Trust and safety operator<br/>Reports, KYB, moderation and abuse]

    zoohelp((ZooHelp<br/>Animal rescue coordination<br/>feed, chat, notifications, trust))
    push[[FCM / APNs<br/>Mobile push delivery]]
    media[[Cloudinary / S3 / R2<br/>Media upload and delivery]]
    payment[[PSP, future optional<br/>Community support payments after scale]]

    user -->|HTTPS / WebSocket| zoohelp
    volunteer -->|HTTPS / WebSocket| zoohelp
    ong -->|HTTPS| zoohelp
    admin -->|HTTPS| zoohelp
    zoohelp -->|push jobs| push
    zoohelp -->|upload intents| media
    zoohelp -->|disabled until business phase| payment
```

## C4 Container

```mermaid
flowchart TB
    user[Mobile user] --> mobile[Expo mobile app<br/>React Native<br/>Emergency, feed, chat, map, outbox]
    admin[Admin operator] --> adminweb[Admin web<br/>React<br/>Users, moderation, ONG/KYB, support]

    mobile -->|REST / WebSocket| api[Rust API<br/>Axum / Tokio<br/>Auth, feed, rescue, chat, geo, notifications, trust]
    adminweb -->|REST| api

    api -->|authoritative writes| pg[(PostgreSQL<br/>users, posts, chat, rescue, audit, subscriptions)]
    api -->|cache, rate limit, hot lookup| redis[(Redis<br/>production scale target)]
    api -->|domain events| nats[(NATS or Kafka<br/>event bus target)]
    api -->|media intents| media[[Cloudinary / S3 / R2]]

    nats --> worker[Rust workers<br/>push fanout, retries, DLQ]
    nats --> ai[Python AI workers<br/>moderation, fraud, analytics]
    worker --> push[[FCM / APNs]]
```

## Rescue Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Draft
    Draft --> QueuedLocalOutbox: user submits offline or weak network
    Draft --> Created: API accepts rescue/post
    QueuedLocalOutbox --> Created: retry with idempotency key
    Created --> AlertQueued: emergency or urgent with GPS
    AlertQueued --> FanoutRunning: notification job claimed
    FanoutRunning --> Notified: nearby users/ONGs selected
    Notified --> ChatOpen: responders coordinate
    ChatOpen --> InProgress: responder confirms action
    InProgress --> Safe: animal secured
    InProgress --> Escalated: needs NGO/vet/admin help
    Escalated --> Safe
    Safe --> Closed
    Created --> RejectedAbuse: trust and safety action
    Notified --> RejectedAbuse
    RejectedAbuse --> Closed
    Closed --> [*]
```

## Rescue Creation Sequence

```mermaid
sequenceDiagram
    participant M as Mobile app
    participant API as Rust API
    participant DB as PostgreSQL
    participant Bus as Redis/NATS target
    participant W as Push worker
    participant P as FCM/APNs
    participant C as Chat/WebSocket

    M->>API: POST /v1/posts or /v1/rescue/sessions with GPS
    API->>API: validate JWT, payload, media, urgency
    API->>DB: insert post/rescue with immediate visibility
    API->>DB: create audit/moderation job metadata
    API->>Bus: publish rescue.created
    API-->>M: 201 + rescue status + idempotency result
    Bus->>W: rescue.created
    W->>DB: select nearby push subscriptions
    W->>P: send critical rescue push
    W->>DB: persist delivery status
    M->>C: open rescue/chat WebSocket
    C-->>M: realtime updates and chat messages
```

## Notification Flow

```mermaid
flowchart LR
    A[Emergency post accepted] --> B[rescue.created event]
    B --> C[Fanout worker]
    C --> D{Recipient lookup}
    D -->|PostGIS enabled later| E[ST_DWithin geography index]
    D -->|Current cloud fallback| F[lat/lng bounding box + haversine]
    E --> G[Dedupe and rate limit]
    F --> G
    G --> H[Persist notification row]
    H --> I[Send FCM/APNs push]
    I --> J{Delivery result}
    J -->|success| K[delivery_status=sent]
    J -->|temporary failure| L[retry with backoff]
    J -->|permanent failure| M[DLQ and token cleanup]
```

## Event Flow

```mermaid
flowchart TB
    API[Rust API] -->|post.created| Bus[(Redis/NATS event bus)]
    API -->|chat.message.created| Bus
    API -->|report.created| Bus
    API -->|media.uploaded| Bus
    Bus --> Push[Push fanout worker]
    Bus --> Realtime[Realtime bridge]
    Bus --> Moderation[Moderation worker]
    Bus --> Audit[Audit sink]
    Push --> Notifications[(notifications and delivery attempts)]
    Realtime --> Sockets[Connected WebSocket clients]
    Moderation --> Jobs[(moderation_jobs)]
    Audit --> Events[(audit_events)]
```

## Chat Realtime Flow

```mermaid
sequenceDiagram
    participant A as User A mobile
    participant API as Rust API
    participant DB as PostgreSQL
    participant Bus as Redis/NATS target
    participant B as User B mobile

    A->>API: WebSocket /v1/chat/rooms/:id/ws?access_token=...
    B->>API: WebSocket /v1/chat/rooms/:id/ws?access_token=...
    A->>API: text message
    API->>DB: persist chat_messages
    API->>Bus: publish chat.message.created
    Bus-->>API: fanout to subscribed API replicas
    API-->>A: delivery event
    API-->>B: delivery event
    B->>API: read receipt
    API->>DB: persist read receipt
```

## Benchmark Evidence Flow

```mermaid
flowchart LR
    Code[Git SHA] --> Run[Benchmark run]
    Infra[Infra topology] --> Run
    Data[Seeded dataset] --> Run
    Run --> Raw[k6/Locust/Vegeta raw output]
    Run --> Metrics[Prometheus/Grafana metrics]
    Raw --> Report[benchmarks/reports/*.md]
    Metrics --> Report
    Report --> Claim{Public claim allowed?}
    Claim -->|p95/error rate documented| Yes[Use in README/recruiting]
    Claim -->|missing raw proof| No[Keep as target only]
```
