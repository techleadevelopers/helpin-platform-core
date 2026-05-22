-- Future PostGIS benchmark.
-- Enable only after the cloud plan supports PostGIS and the schema has a geography(Point, 4326) column.
--
-- Expected column:
-- posts.location_geog geography(Point, 4326)
--
-- Usage:
-- psql "$env:DATABASE_URL" -v lat=-23.5505 -v lng=-46.6333 -v radius_m=25000 -f .\benchmarks\sql\geo-query-postgis.sql

EXPLAIN (ANALYZE, BUFFERS, VERBOSE)
WITH origin AS (
  SELECT ST_SetSRID(ST_MakePoint(:'lng'::double precision, :'lat'::double precision), 4326)::geography AS point
)
SELECT
  p.id,
  p.title,
  p.urgent,
  ST_Distance(p.location_geog, origin.point) / 1000.0 AS distance_km
FROM posts p
CROSS JOIN origin
WHERE p.location_geog IS NOT NULL
  AND ST_DWithin(p.location_geog, origin.point, :'radius_m'::double precision)
ORDER BY p.urgent DESC, distance_km ASC, p.created_at DESC
LIMIT 100;
