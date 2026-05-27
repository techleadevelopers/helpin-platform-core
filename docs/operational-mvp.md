# ZooHelp Operational MVP

This document keeps the first operational scope clear: validate real rescue coordination before expanding into heavier automation.

## Product Thesis

ZooHelp is not only a feed, adoption app, or donation surface. The core value is a trusted coordination system for animal rescue:

```text
simple rescue report -> verified local network -> nearby response -> measured outcome
```

Trust is part of the infrastructure. Without it, the platform is exposed to fake NGOs, fraud, spam, emotional exploitation, and low-quality emergency reports.

## Rapid Community Response

The operational loop is intentionally direct:

```text
vulnerable animal
-> user posts photo, description, urgency, and location
-> backend validates geolocation and classifies emergency intent
-> durable fanout state starts at phase 1
-> nearby NGOs, volunteers, and trusted community members are selected by operational score
-> push notification jobs are created with high urgency and a deep link
-> confirmed helper response pauses aggressive expansion
-> people coordinate through the post, map action, and chat
-> the case receives an outcome
```

The push layer must behave like targeted emergency fanout, not generic social engagement. The right behavior is:

- send fast after the rescue post is committed
- prefer very nearby subscribers over broad broadcast
- use a phase-1 urgent rescue radius of `300 m`
- expand progressively only while nobody confirmed `Estou indo`
- respect each subscriber's configured radius
- use critical-alert opt-in for urgent/emergency delivery
- deduplicate by rescue case
- respect fatigue/cooldown to avoid alert spam
- include a direct deep link to route or chat
- measure fanout phase, queue age, failures, and response time

This turns geolocation and notification delivery into the core operating system for local animal rescue.

## Progressive Fanout MVP

The production MVP uses progressive operational fanout instead of one fixed-radius blast.

| Phase | Radius / Target | Delay | Intent |
|-------|-----------------|-------|--------|
| 1 | `0.3 km` | `90s` | sniper local, recent/critical-alert users |
| 2 | `0.7 km` | `120s` | expand if no one confirmed |
| 3 | `1.0 km` | `180s` | neighborhood response |
| 4 | `3.0 km` | `300s` | broader nearby response |
| 5 | ONG/verified/provider | `300s` | escalation to trusted actors |
| 6 | `10 km` specialists | `300s` | local specialist search |
| 7 | `30 km` specialists | `600s` | regional specialist search |
| 8 | `100 km` specialists | `900s` | state-level specialist search |
| 9 | `300 km` agencies/specialists | `1800s` | environmental agency / rare-case escalation |

Candidate ranking must optimize for expected response, not only proximity:

- distance from the rescue
- recent app/subscription activity
- user trust score when available
- role bonus for ONG, provider, verified or volunteer
- rescue history when available
- critical-alert opt-in
- fatigue penalty for too many recent alerts

`Estou indo` is an operational response, not a resolution. It should create or update `rescue_responses`, increment the public helper count, and pause aggressive expansion. The case remains open until explicitly resolved, cancelled, or completed by the appropriate flow.

After local fanout is exhausted, the system must not broadcast blindly to everyone. It enters specialist escalation:

- `rescue_specialist_providers` stores CETAS, IBAMA, environmental police, fire department, wildlife rescue, marine rescue, rural rescue, vets and verified NGOs.
- providers declare `animal_scopes` such as `dog`, `cat`, `wildlife`, `bird`, `marine`, `livestock`, `reptile` or `general`.
- `rescue_escalation_attempts` records each escalation phase, strategy, radius, candidate count and contacted count.
- if there is no specialist registry match yet, fallback is limited to verified/ONG/vet/admin users with recent push subscriptions. It never falls back to generic unverified broadcast.

This distinction matters operationally:

```text
fanout local -> who is close enough to help fast
specialist escalation -> who is competent enough to solve the case
```

For animals outside the common dog/cat flow, such as birds, wildlife, marine animals, reptiles, livestock or road/rural cases, specialist escalation is the path that makes the system useful in distant places.

Public labels should keep urgency alive:

| State | Label |
|-------|-------|
| no confirmed helper | `Precisa de ajuda` |
| one confirmed helper | `1 pessoa a caminho` |
| multiple confirmed helpers | `{n} pessoas a caminho` |
| someone arrived | `Ajuda no local` |
| fallback active coordination | `Resgate em coordenação` |

Avoid labels such as `Em atendimento` for open cases because they can imply the problem is already covered.

## User Experience Flow

The emergency UX should stay minimal and operational:

1. Primary feed action: `Acionar resgate agora`.
2. Compose asks only for the essentials: photo, short description, GPS, and urgency.
3. Backend creates the post and starts targeted fanout.
4. The reporter lands on rescue status, not back on the generic feed.
5. Recipients open the notification directly into the rescue case.
6. The rescue case exposes two immediate actions: route and chat.
7. The case ends with a clear outcome: resolved, transferred, monitored, or failed.

The interface should avoid explaining the system while the user is under stress. The screen should answer only:

- where is the animal?
- what happened?
- who is responding?
- how do I get there?
- where do I coordinate?
- what is the current status?

## Current Trust Direction

The platform already has the shape for a manual NGO verification workflow:

- NGO profile registration
- address and operational data collection
- pending manual review status
- admin approval, rejection, or block
- KYB document records for uploaded evidence
- reviewer metadata and rejection reason
- public NGO visibility only after approval
- trust score surface
- moderation and report surfaces

The intended first review checklist is:

| Evidence | Purpose |
|----------|---------|
| CNPJ or organization document | legal/organizational identity |
| front and back identity document | responsible person's identity |
| selfie holding the document | proof of possession |
| address and city/state | operational location |
| manual review | fraud reduction |
| rejection/block reason | auditability |

This is a lightweight KYB/KYC-style process adapted to NGOs, rescuers, and animal protection operations.

## MVP Scope

The first public validation should stay narrow:

- create an urgent rescue case quickly
- attach photo, description, urgency, and geolocation
- notify or expose the case to nearby users/volunteers/NGOs
- coordinate response through feed/chat/admin flow
- allow only verified NGOs to appear as trusted institutional actors
- record the operational outcome

The first goal is not to launch every intelligence feature. The first goal is to prove that the system can help resolve a real rescue case with less friction and better coordination.

## Real Rescue Test

The most important early evidence is one documented rescue or vulnerability case improved by the app.

Recommended case report:

| Metric | Example |
|--------|---------|
| time to first response | 4 minutes |
| nearby people reached | 27 |
| verified NGO involved | yes/no |
| average distance | 3.2 km |
| time to rescue resolution | 22 minutes |
| final status | resolved / transferred / monitored |

This turns the product claim into operational proof.

## Later Roadmap

These features should remain roadmap items until the basic rescue loop is validated:

- NGO verified tiers
- SOS prioritization
- rescue analytics
- volunteer reputation
- AI moderation
- animal recognition
- duplicate incident detection
- emergency escalation
- abandonment and rescue heatmaps
- fraud model experiments
- automated trust scoring

They are coherent future layers, but they should not block the first real rescue pilot.

## Sustainability

The product should stay free to use during validation. If the platform reaches meaningful scale, such as hundreds of thousands of users, a transparent maintenance contribution can be considered only to keep infrastructure, storage, notifications, observability, and moderation operations running.

That contribution should be framed as infrastructure sustainability, not as charging people for emergency help.
