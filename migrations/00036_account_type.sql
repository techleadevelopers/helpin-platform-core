DO $$
BEGIN
  -- CREATE TYPE account_type AS ENUM com os valores REAIS já existentes
EXCEPTION
  WHEN duplicate_object THEN NULL;
END $$;