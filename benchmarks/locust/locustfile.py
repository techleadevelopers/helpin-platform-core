import os
import random

from locust import HttpUser, between, task


LAT = os.getenv("LAT", "-23.5505")
LNG = os.getenv("LNG", "-46.6333")
RADIUS_KM = os.getenv("RADIUS_KM", "25")
ACCESS_TOKEN = os.getenv("ACCESS_TOKEN", "")


class ZooHelpReadPathUser(HttpUser):
    wait_time = between(0.25, 1.5)

    def auth_headers(self):
        if not ACCESS_TOKEN:
            return {}
        return {"Authorization": f"Bearer {ACCESS_TOKEN}"}

    @task(8)
    def feed_nearby(self):
        self.client.get(
            "/v1/feed",
            params={
                "lat": LAT,
                "lng": LNG,
                "radius_km": RADIUS_KM,
                "limit": random.choice([10, 20, 30]),
            },
            name="/v1/feed nearby",
        )

    @task(5)
    def geo_nearby(self):
        self.client.get(
            "/v1/geo/nearby",
            params={
                "lat": LAT,
                "lng": LNG,
                "radius_km": RADIUS_KM,
                "limit": random.choice([10, 20, 30]),
            },
            name="/v1/geo/nearby",
        )

    @task(4)
    def search_city_or_rescue(self):
        query = random.choice(["Campinas", "resgate", "adoção", "cachorro", "gato"])
        self.client.get("/v1/search", params={"q": query, "limit": 20}, name="/v1/search")

    @task(2)
    def health_and_readiness(self):
        self.client.get("/healthz", name="/healthz")
        self.client.get("/readyz", name="/readyz")

    @task(1)
    def notifications_if_authenticated(self):
        if ACCESS_TOKEN:
            self.client.get("/v1/notifications", headers=self.auth_headers(), name="/v1/notifications")
