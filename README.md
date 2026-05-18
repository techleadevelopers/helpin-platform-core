# <img src="https://res.cloudinary.com/limpeja/image/upload/v1779070877/Gemini_Generated_Image_v5ufmcv5ufmcv5uf_rlkxk4.png" alt="ZooHelp Logo" width="68" align="center"> ZooHelp Hybrid Core Platform

### Enterprise-Grade Global Animal Rescue Infrastructure

ZooHelp Hybrid Core is the high-performance backend infrastructure powering ZooHelp’s global ecosystem for animal rescue, adoption, NGO networking, trust systems, geolocation, social marketplace operations, and large-scale community protection.

Built for:
- Low latency
- High concurrency
- Global expansion
- Trust-critical systems
- Marketplace scalability
- Institutional resilience
- Future enterprise NGO infrastructure

---

# Core Mission

To provide scalable, secure, and globally deployable backend infrastructure for modern animal protection ecosystems.

This platform is designed to support:
- Rescue operations
- Adoption systems
- NGO collaboration
- Real-time geolocation
- Community trust scoring
- Donation systems
- Fraud prevention
- AI-assisted moderation

---

# Architecture Overview

## Hybrid Infrastructure Model

### Rust Core Platform
Primary backend optimized for:
- Ultra-low latency
- High throughput
- Memory safety
- API scalability
- Event-driven infrastructure
- Mission-critical business logic

---

### Python Intelligence Layer
Dedicated AI/ML systems for:
- Image moderation
- NLP pipelines
- Recommendation models
- Fraud detection models
- Content classification
- Analytics
- Internal automation
- Operational tooling

---

# Technology Stack

## Core Backend
- Rust
- Axum
- Tokio
- SQLx
- PostgreSQL
- PostGIS
- Redis
- Kafka / NATS
- Docker
- Cloudflare
- OpenAPI
- JWT/Auth
- WebSockets

---

## Intelligence Layer
- Python
- FastAPI
- PyTorch / TensorFlow
- OpenCV
- NLP pipelines
- Celery / background workers
- Fraud analytics
- Recommendation engines

---

# Local Development

## Requirements
- Rust toolchain
- Docker
- Docker Compose
- PostgreSQL/PostGIS
- Redis
- Python 3.11+

---

## Run Locally

```bash
cp .env.example .env
docker compose up -d
cargo run