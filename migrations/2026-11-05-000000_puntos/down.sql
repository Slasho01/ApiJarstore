DELETE FROM configuracion WHERE clave = 'puntos';
ALTER TABLE ordenes DROP CONSTRAINT ordenes_puntos_check;
ALTER TABLE ordenes DROP COLUMN puntos_ganados;
ALTER TABLE ordenes DROP COLUMN descuento_puntos;
ALTER TABLE ordenes DROP COLUMN puntos_usados;
DROP TABLE IF EXISTS puntos_movimientos;
