-- Configuración SMTP editable desde el panel (respaldada por variables de entorno)
INSERT INTO configuracion (clave, valor) VALUES ('smtp', '{
    "activo": false,
    "host": "",
    "port": 587,
    "security": "starttls",
    "usuario": "",
    "password": "",
    "from_nombre": "JarStore",
    "from_email": ""
}'::jsonb) ON CONFLICT (clave) DO NOTHING;
