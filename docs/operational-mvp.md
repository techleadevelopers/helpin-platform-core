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
-> nearby NGOs, volunteers, and trusted community members are selected
-> push notification is sent with high urgency and a deep link
-> people coordinate through the post, map action, and chat
-> the case receives an outcome
```

The push layer must behave like targeted emergency fanout, not generic social engagement. The right behavior is:

- send fast after the rescue post is committed
- prefer very nearby subscribers over broad broadcast
- use a phase-1 urgent rescue radius of `30 m`
- respect each subscriber's configured radius
- use critical-alert opt-in for urgent/emergency delivery
- deduplicate by rescue case
- include a direct deep link to route or chat
- measure delivery queue age, failures, and response time

This turns geolocation and notification delivery into the core operating system for local animal rescue.

## User Experience Flow

The emergency UX should stay minimal and operational:

1. Primary feed action: `Acionar resgate agora`.
2. Compose asks only for the essentials: photo, short description, GPS, and urgency.
3. Backend creates the post and dispatches the targeted rescue alert.
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
