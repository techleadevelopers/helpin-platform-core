ALTER TABLE users ADD COLUMN IF NOT EXISTS gender text;

ALTER TABLE users DROP CONSTRAINT IF EXISTS users_gender_check;

ALTER TABLE users
  ADD CONSTRAINT users_gender_check
  CHECK (gender IS NULL OR gender IN ('male', 'female'));
