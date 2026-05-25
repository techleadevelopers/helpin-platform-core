import asyncio
import logging
from collections import Counter
from enum import Enum
from time import perf_counter
from typing import Callable, TypeVar, Awaitable

from fastapi import FastAPI, Request
from pydantic import BaseModel, Field, HttpUrl

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
logger = logging.getLogger("zoohelp-ai-workers")

app = FastAPI(title="ZooHelp AI Workers", version="0.1.0")
metrics = Counter()
T = TypeVar("T")

class ModerationDecision(str, Enum):
    approved = "approved"
    needs_review = "needs_review"
    rejected = "rejected"

class ImageModerationRequest(BaseModel):
    post_id: str = Field(min_length=1)
    image_url: HttpUrl

class ImageModerationResponse(BaseModel):
    post_id: str
    decision: ModerationDecision
    risk_score: int = Field(ge=0, le=100)
    labels: list[str]

class TextClassificationRequest(BaseModel):
    post_id: str = Field(min_length=1)
    text: str = Field(min_length=1, max_length=4000)

class TextClassificationResponse(BaseModel):
    post_id: str
    risk_score: int = Field(ge=0, le=100)
    labels: list[str]

@app.middleware("http")
async def observe_requests(request: Request, call_next):
    started = perf_counter()
    metrics["requests_total"] += 1
    try:
        response = await call_next(request)
        metrics[f"responses_{response.status_code}"] += 1
        return response
    finally:
        elapsed_ms = (perf_counter() - started) * 1000
        logger.info("path=%s method=%s elapsed_ms=%.2f", request.url.path, request.method, elapsed_ms)

async def with_retry(operation: Callable[[], Awaitable[T]], attempts: int = 3, base_delay: float = 0.1) -> T:
    last_error: Exception | None = None
    for attempt in range(1, attempts + 1):
        try:
            return await operation()
        except Exception as error:  # pragma: no cover - defensive retry wrapper
            last_error = error
            metrics["retries_total"] += 1
            if attempt == attempts:
                break
            await asyncio.sleep(base_delay * attempt)
    raise RuntimeError("operation failed after retries") from last_error

@app.get("/healthz")
def healthz() -> dict[str, str]:
    return {"status": "ok", "service": "zoohelp-ai-workers"}

@app.get("/readyz")
def readyz() -> dict[str, bool | str]:
    return {"status": "ready", "model_loaded": True, "queue_configured": True}

@app.get("/metrics")
def read_metrics() -> dict[str, int]:
    return dict(metrics)

@app.post("/v1/moderate-image")
async def moderate_image(payload: ImageModerationRequest) -> ImageModerationResponse:
    async def classify() -> ImageModerationResponse:
        return ImageModerationResponse(
            post_id=payload.post_id,
            decision=ModerationDecision.needs_review,
            risk_score=25,
            labels=["animal", "pending_model"],
        )

    return await with_retry(classify)

@app.post("/v1/classify-text")
def classify_text(payload: TextClassificationRequest) -> TextClassificationResponse:
    text = payload.text.lower()
    labels: list[str] = []
    score = 0
    markers = {
        "pix": "payment_risk",
        "recompensa": "reward",
        "urgente": "urgent",
        "fora da plataforma": "off_platform",
        "maus-tratos": "abuse_report",
    }
    for marker, label in markers.items():
        if marker in text:
            labels.append(label)
            score += 15

    return TextClassificationResponse(
        post_id=payload.post_id,
        risk_score=min(score, 100),
        labels=labels or ["neutral"],
    )
