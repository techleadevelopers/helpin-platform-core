-- Current cloud-compatible geo benchmark.
-- Uses latitude/longitude columns and a bounding box before distance scoring.
--
-- Usage:
-- psql "$env:DATABASE_URL" -v lat=-23.5505 -v lng=-46.6333 -v radius_km=25 -f .\benchmarks\sql\geo-query-fallback.sql

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
WITH params AS (
  SELECT
    :'lat'::double precision AS lat,
    :'lng'::double precision AS lng,
    :'radius_km'::double precision AS radius_km
),
box AS (
  SELECT
    lat,
    lng,
    radius_km,
    radius_km / 111.045 AS lat_delta,
    radius_km / (111.045 * GREATEST(cos(radians(lat)), 0.01)) AS lng_delta
  FROM params
)
SELECT
  p.id,
  p.title,
  p.urgent,
  p.latitude,
  p.longitude,
  6371.0 * acos(
    LEAST(
      1.0,
      GREATEST(
        -1.0,
        cos(radians(box.lat))
          * cos(radians(p.latitude))
          * cos(radians(p.longitude) - radians(box.lng))
          + sin(radians(box.lat))
          * sin(radians(p.latitude))
      )
    )
  ) AS distance_km
FROM posts p
CROSS JOIN box
WHERE p.latitude IS NOT NULL
  AND p.longitude IS NOT NULL
  AND p.latitude BETWEEN box.lat - box.lat_delta AND box.lat + box.lat_delta
  AND p.longitude BETWEEN box.lng - box.lng_delta AND box.lng + box.lng_delta
ORDER BY p.urgent DESC, distance_km ASC, p.created_at DESC
LIMIT 100;
