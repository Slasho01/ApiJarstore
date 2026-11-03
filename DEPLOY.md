# Despliegue en Render (2 servicios)

Arquitectura: la **API** (este repo, `ApiJarstore`) corre como Web Service en Docker,
y el **frontend** (`PageMain`) como Static Site. Se comunican por HTTP con CORS.

> El plan `free` de Render se **pausa** con inactividad (arranque en frío ~1 min).
> Para una tienda real, sube la API y la base de datos a un plan de pago.

## 0. Subir los archivos a GitHub
En este repo se agregaron `Dockerfile`, `.dockerignore` y `render.yaml`.
En `PageMain` se agregó `render.yaml`. Haz commit y push de ambos.
(El `.gitignore` ya excluye `.env`: **nunca** lo subas.)

## 1. Base de datos + API (Blueprint)
1. En Render: **New → Blueprint** y elige el repo **ApiJarstore**.
2. Render lee `render.yaml` y crea la base **jarstore-db** y el servicio **jarstore-api**.
3. En **jarstore-api → Environment**, completa los secretos (los marcados `sync:false`):
   - `JWT_SECRET`: una cadena larga y aleatoria (p. ej. `openssl rand -hex 32`).
   - `CLOUDINARY_URL`: `cloudinary://API_KEY:API_SECRET@zdxhrtbf`.
   - `MP_ACCESS_TOKEN`, `MP_WEBHOOK_SECRET` (Mercado Pago).
   - SMTP y `BLUEEXPRESS_*` si ya los tienes (opcionales).
   - `API_PUBLIC_URL`, `FRONTEND_URL`, `CORS_ORIGINS`: los ajustas en el paso 3.
4. `DATABASE_URL` y `HOST=0.0.0.0` ya quedan puestos por el blueprint.
   Las **migraciones se aplican solas** al arrancar.
5. Cuando termine, copia la URL de la API, p. ej. `https://jarstore-api.onrender.com`.

## 2. Frontend (Static Site)
1. **New → Blueprint** y elige el repo **PageMain**.
2. En **jarstore-frontend → Environment**, define:
   - `VITE_API_URL` = la URL de la API del paso 1 (**sin** barra final, **sin** `/api`),
     p. ej. `https://jarstore-api.onrender.com`.
   - (Vite incrusta esta variable en el build; si la cambias, hay que reconstruir.)
3. Deploy. Copia la URL del frontend, p. ej. `https://jarstore-frontend.onrender.com`.

## 3. Conectar los dos (CORS y URLs públicas)
En **jarstore-api → Environment**, pon:
- `API_PUBLIC_URL` = la URL de la **API** (necesaria para el webhook de Mercado Pago).
- `FRONTEND_URL`   = la URL del **frontend** (enlaces de correos y back_urls de pago).
- `CORS_ORIGINS`   = la URL del **frontend** (varias separadas por coma si aplica).

Guarda y deja que la API vuelva a desplegar. Listo: el frontend ya puede llamar a la API.

## 4. Primer administrador y datos
La base nueva está vacía. Para crear el admin:
1. Regístrate desde el sitio (crea un usuario normal).
2. En Render: **jarstore-db → Connect** (psql) y ejecuta:
   `UPDATE users SET role='admin' WHERE email='TU_CORREO';`
3. Entra al panel (`/admin`) y configura tienda, monedas, envíos, pagos, páginas, etc.

## Notas
- Webhook de Mercado Pago: queda en `API_PUBLIC_URL` + `/api/v2/pagos/mercadopago/webhook`
  (ya corregido en el código). Como Render da HTTPS, el webhook sí llega (en local no).
- Imágenes: van a **Cloudinary**, así que el disco efímero de Render no es problema.
- Salud: la API responde en `/api/v2/health`.
