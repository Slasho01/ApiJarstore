# JarStore API

API REST de e-commerce en Rust (actix-web + Diesel + PostgreSQL).

## Arranque (desarrollo)

```bash
cp .env.example .env        # y ajusta DATABASE_URL / JWT_SECRET
cargo run
```

Las migraciones están embebidas en el binario y **se aplican solas al arrancar**, así que no hace falta `diesel_cli`.

Variables (`.env`):

| Variable | Default | Descripción |
|---|---|---|
| `DATABASE_URL` | — | obligatoria |
| `JWT_SECRET` (o `WT_SECRET_KEY`) | — | obligatoria, mínimo 16 caracteres |
| `JWT_TTL_SECONDS` | 7200 | duración del token |
| `HOST` / `PORT` | 127.0.0.1 / 8080 | |
| `CORS_ORIGINS` | (vacío = cualquiera) | separados por coma |
| `DB_POOL_SIZE` | 10 | |
| `RUST_LOG` | info | |
| `FRONTEND_URL` | http://localhost:5173 | enlaces en los correos |
| `SMTP_HOST`, `SMTP_PORT`, `SMTP_SECURITY`, `SMTP_USER`, `SMTP_PASSWORD`, `SMTP_FROM` | (vacío) | sin `SMTP_HOST` los correos se escriben en el log |

## Producción (un solo servidor)

```bash
cd ../Frontend && npm ci && npm run build        # genera Frontend/dist
cd ../jarstoreapi && cargo build --release
PUBLIC_URL=https://tudominio.cl FRONTEND_DIST=../Frontend/dist ./target/release/jarstoreapi
```

La API sirve la tienda completa y, en cada página, inserta en el HTML inicial:
título, descripción, canonical, Open Graph y datos estructurados JSON-LD
(Organization, WebSite + SearchAction, Product + Offer en CLP, BreadcrumbList).
También genera `/sitemap.xml` (productos, categorías, marcas y páginas) y `/robots.txt`.
Las URL antiguas de productos (por id o slug anterior) redirigen con **301**.
Si vuelves a compilar el frontend, la API lo toma sin reiniciarse.

Pon un proxy con HTTPS delante (Caddy, nginx o el de tu hosting). Para Mercado Pago, el webhook necesita `API_PUBLIC_URL` en https.

### Imágenes
`POST /api/uploads` recibe la imagen (JPG, PNG, WebP o GIF, hasta 10 MB) y la deja lista para la web:
- **Local** (por defecto): WebP de 320, 640, 1024 y 1600 px con nombre descriptivo, servidos en `/uploads` con caché de 1 año.
- **Cloudinary**: define `CLOUDINARY_URL` y listo; los tamaños se entregan con `f_auto,q_auto` (AVIF o WebP según el navegador).

### Mercado Pago (Checkout Pro)
1. Define `MP_ACCESS_TOKEN` (y `MP_WEBHOOK_SECRET`, `API_PUBLIC_URL`).
2. En Admin → Tienda → Medios de pago agrega uno de tipo **Mercado Pago** y márcalo sólo para Chile.
3. Al confirmar, el cliente va a Mercado Pago; al volver, el pago se verifica contra la API de Mercado Pago y el pedido pasa a **pagado**. El webhook hace lo mismo aunque el cliente cierre la pestaña.

Para Venezuela, crea medios **manuales** (Pago Móvil, transferencia…) con sus instrucciones; el admin registra el pago desde el pedido.

### Primer admin

El registro siempre crea usuarios con rol `user`. Para tener el primer admin:

```sql
UPDATE users SET role = 'admin' WHERE username = 'tu_usuario';
```

Después, el admin puede cambiar roles con `PUT /api/admin/users/{id}/role`.

## Convenciones

- Autenticación: `Authorization: Bearer <token>` (se obtiene con `/api/auth/login`).
- Los precios son enteros en la **unidad mínima de la moneda base** (CLP no tiene decimales: `12990` = $12.990). Otras monedas (p. ej. VES) se muestran convertidas con la tasa configurada en el panel.
- Errores siempre en JSON: `{ "error": "mensaje", "code": "NOT_FOUND" }`.
- Listados paginados con `?page=1&per_page=20` → `{ items, page, per_page, total, total_pages }`.
- Se aceptan rutas con o sin `/` final.

## Endpoints

🔓 público · 🔑 cualquier usuario autenticado · 🏪 seller/admin (dueño del producto) · 👑 admin

### Auth y usuarios
| Método | Ruta | | Notas |
|---|---|---|---|
| POST | `/api/auth/register` | 🔓 | `{username, email, password}` (alias: `POST /api/users`) |
| POST | `/api/auth/login` | 🔓 | `{username \| email, password}` → `{token, user, ...}` (alias: `/api/login`). 5 fallos → 429 por 15 min |
| GET | `/api/users/me` | 🔑 | |
| PUT | `/api/users/me/password` | 🔑 | `{old_password, new_password, confirm_new_password}` |
| GET | `/api/users/{id}` | 🔑 | uno mismo o admin |
| GET | `/api/health` | 🔓 | comprueba la BD |

### Perfil
| Método | Ruta | | Notas |
|---|---|---|---|
| POST | `/api/perfil` | 🔑 | `{nombre, apellido, telefono?, direccion?}` |
| GET / PUT | `/api/perfil/mi-perfil` | 🔑 | |

### Categorías
| Método | Ruta | | Notas |
|---|---|---|---|
| GET | `/api/categorias` | 🔓 | admin ve también las inactivas |
| GET | `/api/categorias/{id}` | 🔓 | |
| POST / PUT / DELETE | `/api/categorias[/{id}]` | 👑 | borrar no borra productos (quedan sin categoría) |

### Productos
| Método | Ruta | | Notas |
|---|---|---|---|
| GET | `/api/productos` | 🔓 | filtros: `q, categoria_id, vendedor_id, min_precio, max_precio, en_stock=true, orden=precio_asc\|precio_desc\|nombre\|recientes` |
| GET | `/api/productos/{id}` | 🔓 | incluye variantes, imágenes y calificación |
| GET | `/api/productos/mios` | 🏪 | incluye inactivos |
| POST | `/api/productos` | 🏪 | `{nombre, precio, stock, descripcion?, sku?, estatus?, categoria_id?, variantes?: [...], imagenes?: [...]}` |
| PUT / DELETE | `/api/productos/{id}` | 🏪 | sólo el dueño o admin |
| POST | `/api/productos/{id}/variantes` | 🏪 | `{nombre, valor, stock, precio_extra?}` |
| PUT / DELETE | `/api/variantes/{id}` | 🏪 | |
| POST | `/api/productos/{id}/imagenes` | 🏪 | `{image_url, alt_text?, is_main?}` |
| POST | `/api/variantes/{id}/imagenes` | 🏪 | |
| DELETE | `/api/imagenes/{id}` · `/api/variantes/imagenes/{id}` | 🏪 | |

### Reseñas
| Método | Ruta | | Notas |
|---|---|---|---|
| GET | `/api/productos/{id}/resenas` | 🔓 | paginado |
| POST | `/api/productos/{id}/resenas` | 🔑 | `{calificacion 1-5, comentario?}` · sólo si compraste el producto (orden pagada/enviada/entregada) · una por producto |
| PUT / DELETE | `/api/resenas/{id}` | 🔑 | autor (admin también puede borrar) |

### Carrito
| Método | Ruta | | Notas |
|---|---|---|---|
| GET | `/api/carrito` | 🔑 | líneas con precio, subtotal y disponibilidad + total |
| POST | `/api/carrito` | 🔑 | `{producto_id, variante_id?, cantidad?}` · si ya existe, suma · valida stock |
| PUT | `/api/carrito/{linea_id}` | 🔑 | `{cantidad}` (≤ 0 elimina) |
| DELETE | `/api/carrito/{linea_id}` · `/api/carrito` | 🔑 | quitar línea · vaciar |

### Órdenes
| Método | Ruta | | Notas |
|---|---|---|---|
| POST | `/api/ordenes` | 🔑 | checkout. `{direccion_id \| direccion_envio, metodo_pago?, cupon?, items?}` · sin `items` usa el carrito y lo vacía · **los precios los calcula el servidor** |
| GET | `/api/ordenes` | 🔑 | mis órdenes |
| GET | `/api/ordenes/{id}` | 🔑 | con detalles, pagos y envío |
| POST | `/api/ordenes/{id}/cancelar` | 🔑 | el dueño si está `pendiente`; admin también si está `pagado`. Devuelve el stock |

Estados: `pendiente → pagado → enviado → entregado`, y `pendiente|pagado → cancelado`.

### Wishlist y direcciones
| Método | Ruta | | Notas |
|---|---|---|---|
| GET / POST | `/api/wishlist` | 🔑 | POST `{producto_id}` |
| DELETE | `/api/wishlist/{producto_id}` | 🔑 | |
| GET / POST | `/api/direcciones` | 🔑 | `{direccion, titulo?, ciudad?, estado?, codigo_postal?, pais?, telefono?}` |
| PUT / DELETE | `/api/direcciones/{id}` | 🔑 | |

### Cupones
| Método | Ruta | | Notas |
|---|---|---|---|
| GET | `/api/cupones/{codigo}/validar` | 🔑 | calcula el descuento sobre tu carrito actual |
| GET / POST | `/api/admin/cupones` | 👑 | `{codigo, descuento_porcentaje \| descuento_fijo, fecha_expiracion?, uso_maximo?, activo?}` |
| PUT / DELETE | `/api/admin/cupones/{id}` | 👑 | |

### Tienda (configuración y políticas)
| Método | Ruta | | Notas |
|---|---|---|---|
| GET | `/api/config` | 🔓 | datos de la tienda, monedas activas, envíos por país, medios de pago, páginas publicadas |
| GET | `/api/paginas/{slug}` | 🔓 | página publicada (devoluciones, garantía, …) |
| GET | `/api/admin/config` | 👑 | toda la configuración + si el SMTP está configurado |
| PUT | `/api/admin/config/{tienda\|monedas\|envio\|pagos}` | 👑 | se valida antes de guardar |
| GET / POST | `/api/admin/paginas` | 👑 | crear páginas nuevas |
| PUT / DELETE | `/api/admin/paginas/{id}` | 👑 | editar contenido, publicar/ocultar |

### Catálogo: URLs, buscador, marketing y pagos online
| Método | Ruta | | Notas |
|---|---|---|---|
| GET | `/api/productos/{id o slug}` | 🔓 | acepta el slug actual, uno anterior o el id; `slug` en la respuesta es la URL canónica |
| GET | `/api/categorias/{id o slug}` | 🔓 | `{ categoria, ruta }` (ancestros para breadcrumbs) |
| GET | `/api/marcas/{slug}` | 🔓 | |
| GET | `/api/buscar/sugerencias?q=` | 🔓 | autocompletado: productos, categorías y marcas |
| GET / PUT | `/api/productos/{id}/complementos` | 🔓 / 🏪 | `{ sugeridos, comprados_juntos }` · PUT `{ ids: [...] }` |
| POST | `/api/uploads` | 🏪 | multipart `file` (+ `nombre` opcional) |
| GET | `/api/banners?ubicacion=hero\|promo` | 🔓 | sólo los vigentes |
| GET / POST / PUT / DELETE | `/api/admin/banners[/{id}]` | 👑 | con fechas `desde` / `hasta` |
| POST | `/api/newsletter` · `/api/newsletter/confirmar` · `/api/newsletter/baja` | 🔓 | doble confirmación por correo |
| GET | `/api/admin/newsletter` · `/api/admin/newsletter.csv` | 👑 | |
| POST | `/api/ordenes/{id}/pagar` | 🔑 | devuelve `init_point` de Mercado Pago |
| POST | `/api/ordenes/{id}/pago/verificar` | 🔑 | `{ payment_id }` al volver de Mercado Pago |
| POST | `/api/pagos/mercadopago/webhook` | — | notificaciones (firma `x-signature` verificada) |

Filtros extra de `GET /api/productos`: `categoria` (slug, incluye subcategorías), `marca_slug`, `valoracion` (mínima), `orden=relevancia|mas_vendidos|recientes|precio_asc|precio_desc|descuento`.

### Facturación (boleta/factura)
Sólo para Chile (país de entrega CL): en el checkout se elige **boleta** o **factura**; factura exige RUT (validado con dígito verificador), razón social, giro y dirección. Venezuela no pide documento tributario. El admin sube el documento emitido (PDF o imagen) y el cliente lo descarga desde su pedido.

| Método | Ruta | | Notas |
|---|---|---|---|
| POST | `/api/admin/ordenes/{id}/documento` | 👑 | multipart `file` (PDF o imagen, hasta 15 MB) |

En `POST /api/ordenes`: `tipo_documento` (boleta\|factura) y, si factura, `factura_rut`, `factura_razon_social`, `factura_giro`, `factura_direccion`.

### Programa de puntos
Reglas en Admin → Tienda → Programa de puntos (por defecto: 1 punto por cada $1.000 en productos, 1 punto = $10, canje desde 100 puntos y hasta el 50% del pedido).
- Se ganan sobre productos menos cupón y puntos usados (el envío no suma) y se **acreditan cuando el pedido queda pagado**.
- Se canjean en el checkout (`puntos` en `POST /api/ordenes` y en `/api/ordenes/cotizar`). El saldo se bloquea durante la compra, así no se puede gastar dos veces en paralelo.
- Al cancelar un pedido se devuelven los puntos usados y se revierten los ganados.
- El saldo es la suma de un libro de movimientos auditable.

| Método | Ruta | | Notas |
|---|---|---|---|
| GET | `/api/puntos` | 🔑 | saldo, equivalencia, por acreditar e historial |
| GET / POST | `/api/admin/users/{id}/puntos` | 👑 | ver saldo · ajuste `{ puntos: ±n, motivo }` |

### Extras del catálogo y checkout
| Método | Ruta | | Notas |
|---|---|---|---|
| GET | `/api/productos/facetas` | 🔓 | marcas, características y rango de precio (`?categoria_id=&q=`) |
| GET | `/api/productos/mas-vendidos` | 🔓 | por unidades vendidas en pedidos pagados |
| GET | `/api/productos/{id}/relacionados` | 🔓 | misma categoría (o marca) |
| POST | `/api/ordenes/cotizar` | 🔑 | subtotal, descuento, métodos de envío con costo y total, sin crear la orden |
| POST | `/api/auth/forgot` | 🔓 | `{email}` → envía el enlace de recuperación (siempre responde 200) |
| POST | `/api/auth/reset` | 🔓 | `{token, password}` (token de un solo uso, vence en 60 min) |

Filtros nuevos de `GET /api/productos`: `marca=Lenovo,HP`, `oferta=true`, `destacado=true`, `specs={"RAM":["16 GB"]}`, `orden=descuento`.
El checkout (`POST /api/ordenes`) ahora exige `pais`, `nombre_comprador` y `email_comprador`, y acepta `metodo_envio`, `metodo_pago` y `moneda`; el costo de envío se calcula y se cobra en el servidor.

### Admin
| Método | Ruta | | Notas |
|---|---|---|---|
| GET | `/api/admin/users` | 👑 | `?q=&role=` |
| PUT | `/api/admin/users/{id}/role` | 👑 | `{role: "admin"\|"seller"\|"user"}` |
| DELETE | `/api/admin/users/{id}` | 👑 | |
| GET | `/api/admin/perfiles` · `/api/admin/perfiles/{user_id}` | 👑 | |
| GET | `/api/admin/ordenes` | 👑 | `?estatus=&user_id=` |
| PUT | `/api/admin/ordenes/{id}/estatus` | 👑 | `{estatus}` · valida transiciones |
| POST | `/api/admin/ordenes/{id}/pagos` | 👑 | `{monto, estatus: completado\|pendiente\|fallido, metodo?, referencia_externa?}` · si los pagos completados cubren el total → `pagado` |
| PUT | `/api/admin/ordenes/{id}/envio` | 👑 | `{empresa_envio?, codigo_seguimiento?, estatus?, fecha_envio?, fecha_entrega?}` |

## Pruebas

`scripts/smoke_test.py` recorre la API completa (275 comprobaciones: compras concurrentes, subida de imágenes, SEO del HTML, Mercado Pago contra un servidor simulado…). **Usa una BD de prueba**, porque crea datos:

```bash
createdb jarstoreapi_test
DATABASE_URL=postgres://.../jarstoreapi_test PORT=18080 cargo run &
python3 scripts/smoke_test.py http://127.0.0.1:18080 postgres://.../jarstoreapi_test
```

## Estructura

```
src/
  main.rs         servidor, middlewares (CORS, logs, rutas sin "/" final, errores JSON)
  config.rs       variables de entorno
  db.rs           pool, migraciones embebidas, db::run (Diesel fuera del hilo async)
  errors.rs       ApiError → respuesta JSON
  auth.rs         JWT, extractor AuthUser, limitador de login
  routes.rs       mapa de rutas
  controller/     handlers HTTP (permisos + JSON)
  services/       lógica de negocio y consultas
  models/         structs de Diesel
migrations/       SQL
```
