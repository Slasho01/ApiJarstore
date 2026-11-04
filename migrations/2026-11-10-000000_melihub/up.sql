-- Vincula cada producto de JarStore con su origen en MeliHub (hub de Mercado Libre).
-- Permite importar publicaciones y reportar ventas para sincronizar stock en ambos lados.
ALTER TABLE productos ADD COLUMN melihub_id UUID;
CREATE UNIQUE INDEX productos_melihub_id_uidx ON productos (melihub_id) WHERE melihub_id IS NOT NULL;
