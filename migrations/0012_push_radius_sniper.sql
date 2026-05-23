ALTER TABLE push_subscriptions
  DROP CONSTRAINT IF EXISTS push_subscriptions_radius_km_check;

ALTER TABLE push_subscriptions
  DROP CONSTRAINT IF EXISTS push_subscriptions_radius_km_range;

ALTER TABLE push_subscriptions
  ADD CONSTRAINT push_subscriptions_radius_km_range
  CHECK (radius_km BETWEEN 0.03 AND 50);
