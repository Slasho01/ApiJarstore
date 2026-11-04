DROP INDEX IF EXISTS productos_melihub_id_uidx;
ALTER TABLE productos DROP COLUMN IF EXISTS melihub_id;
