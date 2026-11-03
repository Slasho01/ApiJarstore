#!/usr/bin/env python3
"""
Prueba de humo end-to-end de la API (sólo librería estándar).

Uso:
    1. Levanta la API apuntando a una BD de PRUEBA (crea datos).
    2. python3 scripts/smoke_test.py http://127.0.0.1:8080 "postgres://.../jarstoreapi_test"
       (la URL de la BD se usa sólo con psql para promover usuarios a admin/seller)
"""
import json, subprocess, sys, uuid, urllib.request, urllib.error

BASE = (sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8080").rstrip("/") + "/api/v2"
DB = sys.argv[2] if len(sys.argv) > 2 else None
LOG = sys.argv[3] if len(sys.argv) > 3 else None
MP_MOCK = sys.argv[4] if len(sys.argv) > 4 else None   # servidor simulado de Mercado Pago (opcional)
MP_SECRET = sys.argv[5] if len(sys.argv) > 5 else None  # log de la API (para leer el correo de recuperación en modo desarrollo)
BUYER = {"pais": "CL", "nombre_comprador": "Ana López", "email_comprador": "ana@test.com"}
passed = failed = 0


def call(method, path, body=None, token=None):
    req = urllib.request.Request(BASE + path, method=method)
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    data = json.dumps(body).encode() if body is not None else None
    try:
        with urllib.request.urlopen(req, data) as r:
            raw = r.read()
            return r.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw)
        except Exception:
            return e.code, raw.decode()


def check(name, cond, info=None):
    global passed, failed
    if cond:
        passed += 1
        print(f"  ok   {name}")
    else:
        failed += 1
        print(f"  FAIL {name} -> {info}")


def expect(name, res, status):
    check(name, res[0] == status, res)
    return res[1]


def psql(sql):
    subprocess.run(["psql", DB, "-qAtc", sql], check=True, capture_output=True)


def new_user(prefix, role=None):
    u = f"{prefix}_{uuid.uuid4().hex[:8]}"
    expect(f"registro {prefix}", call("POST", "/auth/register",
           {"username": u, "email": f"{u}@test.com", "password": "password123"}), 201)
    if role:
        psql(f"UPDATE users SET role='{role}' WHERE username='{u}'")
    tok = call("POST", "/auth/login", {"username": u, "password": "password123"})[1]["token"]
    return u, tok


print("== Salud / auth")
expect("health", call("GET", "/health"), 200)
expect("registro inválido (password corta)", call("POST", "/auth/register",
       {"username": "abc", "email": "a@b.com", "password": "123"}), 400)
admin_name, admin = new_user("admin", "admin")
_, seller = new_user("seller", "seller")
_, seller2 = new_user("seller2", "seller")
buyer_name, buyer = new_user("buyer")
_, buyer2 = new_user("buyer2")
expect("login con password errónea", call("POST", "/auth/login",
       {"username": buyer_name, "password": "malmalmal"}), 401)
expect("login con email", call("POST", "/auth/login",
       {"email": f"{buyer_name}@test.com", "password": "password123"}), 200)
me = expect("GET /users/me", call("GET", "/users/me", token=buyer), 200)
check("me no expone password_hash", "password_hash" not in me, me)
expect("sin token -> 401", call("GET", "/users/me"), 401)
expect("token basura -> 401", call("GET", "/users/me", token="xxx"), 401)
expect("registro duplicado -> 409", call("POST", "/auth/register",
       {"username": buyer_name, "email": "otro@test.com", "password": "password123"}), 409)

print("== Perfil")
expect("crear perfil", call("POST", "/perfil", {"nombre": "Ana", "apellido": "López"}, buyer), 201)
expect("perfil duplicado -> 409", call("POST", "/perfil", {"nombre": "Ana", "apellido": "L"}, buyer), 409)
p = expect("actualizar perfil", call("PUT", "/perfil/mi-perfil", {"telefono": "555"}, buyer), 200)
check("perfil actualizado", p and p["telefono"] == "555", p)
expect("admin lista perfiles", call("GET", "/admin/perfiles", token=admin), 200)
expect("buyer no lista perfiles -> 403", call("GET", "/admin/perfiles", token=buyer), 403)

print("== Categorías")
cat = expect("admin crea categoría", call("POST", "/categorias",
             {"nombre": f"Juguetes {uuid.uuid4().hex[:4]}"}, admin), 201)
expect("seller no crea categoría -> 403", call("POST", "/categorias", {"nombre": "X"}, seller), 403)
expect("categoría duplicada -> 409", call("POST", "/categorias", {"nombre": cat["nombre"].upper()}, admin), 409)
expect("listar categorías (público)", call("GET", "/categorias/"), 200)

print("== Productos")
prod = expect("seller crea producto con variantes e imágenes", call("POST", "/productos", {
    "nombre": "Carrito RC", "precio": 10000, "stock": 5, "categoria_id": cat["id"],
    "variantes": [{"nombre": "Color", "valor": "Rojo", "stock": 2, "precio_extra": 500}],
    "imagenes": [{"image_url": "https://img.test/rc.png", "is_main": True}],
}, seller), 201)
pid, vid = prod["id"], prod["variantes"][0]["id"]
expect("buyer no crea producto -> 403", call("POST", "/productos", {"nombre": "x", "precio": 1, "stock": 1}, buyer), 403)
expect("precio negativo -> 400", call("POST", "/productos", {"nombre": "x", "precio": -1, "stock": 1}, seller), 400)
expect("otro seller no edita -> 403", call("PUT", f"/productos/{pid}", {"precio": 1}, seller2), 403)
expect("dueño edita", call("PUT", f"/productos/{pid}", {"descripcion": "Rápido"}, seller), 200)
lst = expect("catálogo público filtrado", call("GET", f"/productos?q=carrito&categoria_id={cat['id']}&orden=precio_asc"), 200)
check("catálogo incluye producto con imagen", any(i["id"] == pid and i["imagen_principal"] for i in lst["items"]), lst)
expect("orden inválido -> 400", call("GET", "/productos?orden=xyz"), 400)
det = expect("detalle público", call("GET", f"/productos/{pid}"), 200)
check("detalle trae variantes e imágenes", len(det["variantes"]) == 1 and len(det["imagenes"]) == 1, det)
expect("slug inexistente -> 404", call("GET", "/productos/no-existe-este-producto"), 404)
expect("producto inexistente -> 404", call("GET", f"/productos/{uuid.uuid4()}"), 404)
v2 = expect("agregar variante", call("POST", f"/productos/{pid}/variantes",
            {"nombre": "Color", "valor": "Azul", "stock": 1}, seller), 201)
expect("actualizar variante por id", call("PUT", f"/variantes/{v2['id']}", {"stock": 3}, seller), 200)
expect("borrar variante", call("DELETE", f"/variantes/{v2['id']}", token=seller), 204)
hidden = expect("producto inactivo", call("POST", "/productos",
                {"nombre": "Oculto", "precio": 1, "stock": 1, "estatus": False}, seller), 201)
expect("inactivo invisible al público", call("GET", f"/productos/{hidden['id']}"), 404)
expect("inactivo visible al dueño", call("GET", f"/productos/{hidden['id']}", token=seller), 200)
mios = expect("mis productos (seller)", call("GET", "/productos/mios", token=seller), 200)
check("mis productos incluye inactivos", mios["total"] >= 2, mios)

print("== Carrito")
c = expect("agregar al carrito", call("POST", "/carrito", {"producto_id": pid, "cantidad": 2}, buyer), 200)
c = expect("agregar mismo producto suma", call("POST", "/carrito", {"producto_id": pid, "cantidad": 1}, buyer), 200)
check("una sola línea con cantidad 3", len(c["items"]) == 1 and c["items"][0]["cantidad"] == 3, c)
expect("exceder stock -> 400", call("POST", "/carrito", {"producto_id": pid, "cantidad": 10}, buyer), 400)
c = expect("agregar variante", call("POST", "/carrito", {"producto_id": pid, "variante_id": vid, "cantidad": 1}, buyer), 200)
check("total = 3*10000 + 10500", c["total"] == 40500, c)
expect("variante de otro producto -> 400", call("POST", "/carrito",
       {"producto_id": hidden["id"], "variante_id": vid}, buyer), 400)
line = c["items"][0]["id"]
expect("otro usuario no toca mi línea -> 404", call("PUT", f"/carrito/{line}", {"cantidad": 1}, buyer2), 404)
c = expect("actualizar cantidad", call("PUT", f"/carrito/{line}", {"cantidad": 2}, buyer), 200)

print("== Direcciones / cupones")
d = expect("crear dirección", call("POST", "/direcciones",
           {"titulo": "Casa", "direccion": "Calle 1 #2", "ciudad": "CDMX", "pais": "MX"}, buyer), 201)
code = f"PROMO{uuid.uuid4().hex[:4]}"
cup = expect("admin crea cupón 10%", call("POST", "/admin/cupones",
             {"codigo": code.lower(), "descuento_porcentaje": 10, "uso_maximo": 1}, admin), 201)
expect("cupón con ambos descuentos -> 400", call("POST", "/admin/cupones",
       {"codigo": "X1", "descuento_porcentaje": 10, "descuento_fijo": 5}, admin), 400)
val = expect("validar cupón sobre carrito", call("GET", f"/cupones/{code}/validar", token=buyer), 200)
check("descuento = 10% de 30500", val["valido"] and val["descuento"] == 3050, val)

print("== Checkout")
expect("checkout sin dirección -> 400", call("POST", "/ordenes", {**BUYER}, buyer), 400)
o = expect("checkout desde carrito", call("POST", "/ordenes", {**BUYER, "direccion_id": d["id"], "cupon": code}, buyer), 201)
check("total calculado en servidor", o and o["subtotal"] == 30500 and o["descuento"] == 3050 and o["total"] == 27450, o)
check("orden pendiente con 2 detalles", o["estatus"] == "pendiente" and len(o["detalles"]) == 2, o)
check("sin envíos configurados: despacho a coordinar", o["metodo_envio"] == "Despacho a coordinar" and o["costo_envio"] == 0, o)
check("datos del comprador guardados", o["nombre_comprador"] == "Ana López" and o["moneda"] == "CLP", o)
expect("carrito vacío tras checkout", call("GET", "/carrito", token=buyer), 200)
det = call("GET", f"/productos/{pid}")[1]
check("stock descontado (5-2=3, variante 2-1=1)", det["stock"] == 3 and det["variantes"][0]["stock"] == 1, det)
expect("cupón agotado -> 400", call("POST", "/ordenes",
       {**BUYER, "items": [{"producto_id": pid, "cantidad": 1}], "direccion_envio": "X", "cupon": code}, buyer2), 400)
expect("stock insuficiente -> 400", call("POST", "/ordenes",
       {**BUYER, "items": [{"producto_id": pid, "cantidad": 99}], "direccion_envio": "X"}, buyer2), 400)
expect("otro usuario no ve mi orden -> 404", call("GET", f"/ordenes/{o['id']}", token=buyer2), 404)
mine = expect("mis órdenes", call("GET", "/ordenes", token=buyer), 200)
check("mis órdenes = 1", mine["total"] == 1, mine)

print("== Reseñas (requiere compra pagada)")
expect("reseña sin pagar -> 403", call("POST", f"/productos/{pid}/resenas", {"calificacion": 5}, buyer), 403)
o2 = expect("admin registra pago completo", call("POST", f"/admin/ordenes/{o['id']}/pagos",
            {"monto": 27450, "metodo": "tarjeta", "estatus": "completado"}, admin), 201)
check("orden pasa a pagado", o2["estatus"] == "pagado", o2)
r = expect("reseña tras compra", call("POST", f"/productos/{pid}/resenas",
           {"calificacion": 4, "comentario": "Bueno"}, buyer), 201)
expect("reseña duplicada -> 409", call("POST", f"/productos/{pid}/resenas", {"calificacion": 5}, buyer), 409)
expect("calificación fuera de rango -> 400", call("PUT", f"/resenas/{r['id']}", {"calificacion": 9}, buyer), 400)
det = call("GET", f"/productos/{pid}")[1]
check("rating promedio 4", det["calificacion"]["promedio"] == 4.0, det["calificacion"])

print("== Flujo de estados")
expect("transición inválida pagado->entregado -> 409", call("PUT", f"/admin/ordenes/{o['id']}/estatus",
       {"estatus": "entregado"}, admin), 409)
expect("buyer no cancela orden pagada -> 409", call("POST", f"/ordenes/{o['id']}/cancelar", token=buyer), 409)
env = expect("admin registra envío", call("PUT", f"/admin/ordenes/{o['id']}/envio",
             {"empresa_envio": "DHL", "codigo_seguimiento": "123"}, admin), 200)
check("envío visible en orden", env["envio"]["empresa_envio"] == "DHL", env)
expect("pagado -> enviado", call("PUT", f"/admin/ordenes/{o['id']}/estatus", {"estatus": "enviado"}, admin), 200)
o3 = expect("orden explícita (buyer2)", call("POST", "/ordenes",
            {**BUYER, "items": [{"producto_id": pid, "cantidad": 1}], "direccion_envio": "Av. 2"}, buyer2), 201)
expect("buyer2 cancela su orden pendiente", call("POST", f"/ordenes/{o3['id']}/cancelar", token=buyer2), 200)
det = call("GET", f"/productos/{pid}")[1]
check("stock restaurado tras cancelar (3)", det["stock"] == 3, det["stock"])

print("== Wishlist / admin")
expect("wishlist add", call("POST", "/wishlist", {"producto_id": pid}, buyer), 200)
w = expect("wishlist add idempotente", call("POST", "/wishlist", {"producto_id": pid}, buyer), 200)
check("wishlist tiene 1", len(w) == 1, w)
expect("wishlist remove", call("DELETE", f"/wishlist/{pid}", token=buyer), 204)
expect("admin lista órdenes pagadas/enviadas", call("GET", "/admin/ordenes?estatus=enviado", token=admin), 200)
expect("admin lista usuarios", call("GET", f"/admin/users?q={buyer_name}", token=admin), 200)
expect("borrar categoría no borra productos", call("DELETE", f"/categorias/{cat['id']}", token=admin), 204)
expect("producto sigue existiendo", call("GET", f"/productos/{pid}"), 200)
expect("ruta inexistente -> 404 JSON", call("GET", "/nada"), 404)
expect("JSON mal formado -> 400", call("POST", "/carrito", {"cantidad": "x"}, buyer), 400)


print("== Tienda: configuración, páginas, monedas y envíos")
cfg = expect("config pública", call("GET", "/config"), 200)
check("VES sin tasa no se ofrece", [m["codigo"] for m in cfg["monedas"]["lista"]] == ["CLP"], cfg["monedas"])
check("páginas ocultas no salen", cfg["paginas"] == [], cfg["paginas"])
expect("buyer no edita config -> 403", call("PUT", "/admin/config/envio", {"paises": []}, buyer), 403)
adm = expect("admin lee config", call("GET", "/admin/config", token=admin), 200)
check("admin ve estado SMTP", "smtp_configurado" in adm, adm)
expect("config inválida -> 400", call("PUT", "/admin/config/monedas",
       {"base": "CLP", "lista": [{"codigo": "CLP", "nombre": "Peso", "simbolo": "$", "decimales": 0, "tasa": -1}]}, admin), 400)
expect("clave inexistente -> 404", call("PUT", "/admin/config/nada", {}, admin), 404)
expect("activar VES con tasa", call("PUT", "/admin/config/monedas", {"base": "CLP", "lista": [
    {"codigo": "CLP", "nombre": "Peso chileno", "simbolo": "$", "decimales": 0, "tasa": 1},
    {"codigo": "ves", "nombre": "Bolívar", "simbolo": "Bs.", "decimales": 2, "tasa": 0.05}]}, admin), 200)
expect("configurar envíos", call("PUT", "/admin/config/envio", {"paises": [
    {"codigo": "CL", "nombre": "Chile", "metodos": [
        {"id": "estandar", "nombre": "Despacho estándar", "precio": 3990, "gratis_desde": 50000, "plazo": "3-5 días hábiles"},
        {"id": "retiro", "nombre": "Retiro en tienda", "precio": 0}]},
    {"codigo": "VE", "nombre": "Venezuela", "metodos": []}]}, admin), 200)
expect("configurar pagos", call("PUT", "/admin/config/pagos", {"metodos": [
    {"id": "transferencia", "nombre": "Transferencia bancaria", "instrucciones": "Banco X, cuenta 123"}]}, admin), 200)
cfg = call("GET", "/config")[1]
check("VES activo", [m["codigo"] for m in cfg["monedas"]["lista"]] == ["CLP", "VES"], cfg["monedas"])
pages = expect("admin lista páginas", call("GET", "/admin/paginas", token=admin), 200)
dev = next(p for p in pages if p["slug"] == "devoluciones")
expect("página oculta no es pública", call("GET", "/paginas/devoluciones"), 404)
expect("publicar devoluciones", call("PUT", f"/admin/paginas/{dev['id']}",
       {"contenido": "Tienes 10 días para devolver.", "visible": True}, admin), 200)
pg = expect("página pública", call("GET", "/paginas/devoluciones"), 200)
check("contenido publicado", pg["contenido"].startswith("Tienes 10"), pg)
newp = expect("crear página nueva", call("POST", "/admin/paginas", {"slug": "preguntas-frecuentes", "titulo": "FAQ", "visible": True}, admin), 201)
expect("slug inválido -> 400", call("POST", "/admin/paginas", {"slug": "Mal Slug", "titulo": "X"}, admin), 400)
expect("slug repetido -> 409", call("POST", "/admin/paginas", {"slug": "preguntas-frecuentes", "titulo": "X"}, admin), 409)
expect("borrar página", call("DELETE", f"/admin/paginas/{newp['id']}", token=admin), 204)

print("== Catálogo enriquecido")
tech = expect("producto con marca/specs/oferta", call("POST", "/productos", {
    "nombre": "Notebook Pro 14", "precio": 899990, "precio_anterior": 999990, "stock": 4, "marca": "Lenovo",
    "destacado": True, "garantia_meses": 12, "compatibilidad": "Windows 11",
    "especificaciones": [{"nombre": "RAM", "valor": "16 GB"}, {"nombre": "Almacenamiento", "valor": "512 GB SSD"}],
}, seller), 201)
tech2 = expect("otro producto", call("POST", "/productos", {
    "nombre": "Notebook Air 13", "precio": 699990, "stock": 2, "marca": "lenovo",
    "especificaciones": [{"nombre": "RAM", "valor": "8 GB"}]}, seller), 201)
expect("precio_anterior <= precio -> 400", call("POST", "/productos",
       {"nombre": "X", "precio": 100, "precio_anterior": 50, "stock": 1}, seller), 400)
expect("specs repetidas -> 400", call("POST", "/productos", {"nombre": "X", "precio": 1, "stock": 1,
       "especificaciones": [{"nombre": "RAM", "valor": "1"}, {"nombre": "ram", "valor": "2"}]}, seller), 400)
r = expect("filtro marca (insensible a mayúsculas)", call("GET", "/productos?marca=LENOVO"), 200)
check("2 Lenovo", r["total"] == 2, r["total"])
r = expect("filtro oferta", call("GET", "/productos?oferta=true"), 200)
check("sólo la oferta", [i["id"] for i in r["items"]] == [tech["id"]], r)
r = expect("filtro destacado", call("GET", "/productos?destacado=true"), 200)
check("sólo destacado", [i["id"] for i in r["items"]] == [tech["id"]], r)
import urllib.parse
specs = urllib.parse.quote(json.dumps({"RAM": ["16 GB", "32 GB"]}))
r = expect("filtro por característica", call("GET", f"/productos?specs={specs}"), 200)
check("RAM 16 GB", [i["id"] for i in r["items"]] == [tech["id"]], r)
expect("specs mal formado -> 400", call("GET", "/productos?specs=xx"), 400)
f = expect("facetas", call("GET", "/productos/facetas"), 200)
check("faceta marca agrupa Lenovo/lenovo", any(m["total"] == 2 for m in f["marcas"]), f["marcas"])
check("faceta RAM con 2 valores", any(c["nombre"] == "RAM" and len(c["valores"]) == 2 for c in f["caracteristicas"]), f["caracteristicas"])
upd = expect("quitar oferta (null)", call("PUT", f"/productos/{tech['id']}", {"precio_anterior": None, "marca": ""}, seller), 200)
check("oferta y marca quitadas", upd["precio_anterior"] is None and upd["marca"] is None, upd)
expect("poner precio anterior", call("PUT", f"/productos/{tech2['id']}", {"precio_anterior": 799990}, seller), 200)
expect("precio mayor que el anterior -> 400", call("PUT", f"/productos/{tech2['id']}", {"precio": 899990}, seller), 400)
rel = expect("relacionados", call("GET", f"/productos/{pid}/relacionados"), 200)
expect("más vendidos", call("GET", "/productos/mas-vendidos"), 200)

print("== Checkout con envío y moneda")
call("POST", "/carrito", {"producto_id": tech2["id"], "cantidad": 1}, buyer2)
q = expect("cotizar Chile", call("POST", "/ordenes/cotizar", {"pais": "CL", "metodo_envio": "estandar"}, buyer2), 200)
check("envío gratis sobre 50.000", q["costo_envio"] == 0 and q["total"] == 699990 and len(q["metodos_envio"]) == 2, q)
q = expect("cotizar Venezuela", call("POST", "/ordenes/cotizar", {"pais": "VE"}, buyer2), 200)
check("VE sin métodos: a coordinar", q["envio_a_coordinar"] and q["costo_envio"] == 0, q)
expect("país no configurado -> 400", call("POST", "/ordenes/cotizar", {"pais": "AR"}, buyer2), 400)
expect("sin método de envío -> 400", call("POST", "/ordenes", {**BUYER, "direccion_envio": "X", "metodo_pago": "transferencia"}, buyer2), 400)
expect("método de pago inexistente -> 400", call("POST", "/ordenes",
       {**BUYER, "direccion_envio": "X", "metodo_envio": "estandar", "metodo_pago": "bitcoin"}, buyer2), 400)
expect("moneda no disponible -> 400", call("POST", "/ordenes",
       {**BUYER, "direccion_envio": "X", "metodo_envio": "estandar", "metodo_pago": "transferencia", "moneda": "EUR"}, buyer2), 400)
small = expect("producto barato", call("POST", "/productos", {"nombre": "Cable USB-C", "precio": 4990, "stock": 10}, seller), 201)
call("POST", "/carrito", {"producto_id": small["id"], "cantidad": 1}, buyer2)
call("DELETE", "/carrito", token=buyer2)
call("POST", "/carrito", {"producto_id": small["id"], "cantidad": 2}, buyer2)
ov = expect("checkout CL con envío pagado y moneda VES", call("POST", "/ordenes", {**BUYER, "direccion_envio": "Calle 9",
       "metodo_envio": "estandar", "metodo_pago": "transferencia", "moneda": "VES"}, buyer2), 201)
check("total = 9.980 + 3.990 de envío", ov["costo_envio"] == 3990 and ov["total"] == 13970, ov)
check("guarda moneda y tasa", ov["moneda"] == "VES" and ov["tasa_cambio"] == 0.05 and ov["metodo_pago"] == "Transferencia bancaria", ov)

print("== Recuperación de contraseña")
r = expect("forgot (email inexistente igual 200)", call("POST", "/auth/forgot", {"email": "nadie@x.com"}), 200)
expect("forgot email real", call("POST", "/auth/forgot", {"email": f"{buyer_name}@test.com"}), 200)
expect("token inválido -> 400", call("POST", "/auth/reset", {"token": "x" * 64, "password": "nuevaclave123"}), 400)
if LOG:
    import re, time
    time.sleep(0.5)
    tokens = re.findall(r"restablecer\?token=([0-9a-f]{64})", open(LOG).read())
    check("correo de desarrollo en el log", bool(tokens))
    if tokens:
        expect("reset con token", call("POST", "/auth/reset", {"token": tokens[-1], "password": "nuevaclave123"}), 200)
        expect("token de un solo uso", call("POST", "/auth/reset", {"token": tokens[-1], "password": "otraclave123"}), 400)
        expect("login con contraseña nueva", call("POST", "/auth/login", {"username": buyer_name, "password": "nuevaclave123"}), 200)


print("== Categorías jerárquicas y URLs amigables")
root = BASE.rsplit("/api/", 1)[0]  # host sin el prefijo de la API (/api/v2)
comp = expect("categoría padre", call("POST", "/categorias", {"nombre": "Computación"}, admin), 201)
check("slug generado sin tildes", comp["slug"] == "computacion", comp)
nb = expect("subcategoría", call("POST", "/categorias", {"nombre": "Notebooks Gamer", "parent_id": comp["id"]}, admin), 201)
expect("ciclo padre-hijo -> 400", call("PUT", f"/categorias/{comp['id']}", {"parent_id": nb["id"]}, admin), 400)
c = expect("categoría por slug con ruta", call("GET", "/categorias/notebooks-gamer"), 200)
check("ruta = [Computación]", [x["slug"] for x in c["ruta"]] == ["computacion"], c)
gp = expect("producto en subcategoría", call("POST", "/productos", {"nombre": "Notebook Gamer Ñandú 15", "precio": 999990, "stock": 3,
    "categoria_id": nb["id"], "marca": "ASUS ROG", "especificaciones": [{"nombre": "GPU", "valor": "RTX 4060"}]}, seller), 201)
check("slug de producto", gp["slug"] == "notebook-gamer-nandu-15", gp["slug"])
r = expect("filtrar por categoría padre incluye hijas", call("GET", "/productos?categoria=computacion"), 200)
check("producto de la subcategoría aparece", any(i["id"] == gp["id"] for i in r["items"]), r["total"])
f = expect("facetas con subcategorías", call("GET", "/productos/facetas?categoria=computacion"), 200)
check("subcategoría con conteo", any(x["slug"] == "notebooks-gamer" and x["total"] >= 1 for x in f["subcategorias"]), f["subcategorias"])
check("marca con slug", any(m["slug"] == "asus-rog" for m in f["marcas"]), f["marcas"])
expect("producto por slug", call("GET", f"/productos/{gp['slug']}"), 200)
upd = expect("cambiar slug", call("PUT", f"/productos/{gp['id']}", {"slug": "notebook-rog-15"}, seller), 200)
old = expect("slug antiguo sigue resolviendo", call("GET", "/productos/notebook-gamer-nandu-15"), 200)
check("devuelve el slug nuevo (canónico)", old["slug"] == "notebook-rog-15", old["slug"])
expect("slug inválido -> 400", call("PUT", f"/productos/{gp['id']}", {"slug": "Mal Slug"}, seller), 400)
expect("marca por slug", call("GET", "/marcas/asus-rog"), 200)
r = expect("filtro marca_slug", call("GET", "/productos?marca_slug=asus-rog"), 200)
check("1 producto ASUS ROG", r["total"] == 1, r["total"])
expect("borrar padre sube a la hija de nivel", call("DELETE", f"/categorias/{comp['id']}", token=admin), 204)
check("hija queda en la raíz", call("GET", "/categorias/notebooks-gamer")[1]["categoria"]["parent_id"] is None)

print("== Buscador y ordenamientos")
s = expect("sugerencias", call("GET", "/buscar/sugerencias?q=noteb"), 200)
check("sugiere productos y categorías", len(s["productos"]) >= 1 and any(c["slug"] == "notebooks-gamer" for c in s["categorias"]), s)
check("sugerencia trae slug e imagen", "slug" in s["productos"][0] and "imagen" in s["productos"][0], s["productos"][0])
check("q de 1 letra no busca", call("GET", "/buscar/sugerencias?q=n")[1]["productos"] == [])
r = expect("relevancia (por defecto al buscar)", call("GET", "/productos?q=notebook"), 200)
check("empieza por 'Notebook' primero", r["items"][0]["nombre"].lower().startswith("notebook"), [i["nombre"] for i in r["items"]])
r = expect("búsqueda multi-palabra", call("GET", "/productos?q=rog%20gamer"), 200)
check("encuentra por marca + nombre", any(i["id"] == gp["id"] for i in r["items"]), r["total"])
expect("orden más vendidos", call("GET", "/productos?orden=mas_vendidos"), 200)
r = expect("filtro valoración ≥ 4", call("GET", "/productos?valoracion=4"), 200)
check("sólo con reseñas ≥ 4", all((i["calificacion"]["promedio"] or 0) >= 4 for i in r["items"]) and r["total"] >= 1, r["total"])

print("== Complementarios")
cable = expect("accesorio", call("POST", "/productos", {"nombre": "Mouse inalámbrico", "precio": 9990, "stock": 5}, seller), 201)
expect("otro vendedor no edita complementos -> 403", call("PUT", f"/productos/{gp['id']}/complementos", {"ids": [cable["id"]]}, seller2), 403)
expect("auto-complemento -> 400", call("PUT", f"/productos/{gp['id']}/complementos", {"ids": [gp["id"]]}, seller), 400)
cm = expect("definir complementos", call("PUT", f"/productos/{gp['id']}/complementos", {"ids": [cable["id"]]}, seller), 200)
check("complemento guardado", [x["id"] for x in cm["sugeridos"]] == [cable["id"]], cm)
expect("ver complementos (público)", call("GET", f"/productos/{gp['id']}/complementos"), 200)

print("== Banners y newsletter")
expect("buyer no crea banner -> 403", call("POST", "/admin/banners", {"titulo": "X"}, buyer), 403)
b1 = expect("banner vigente", call("POST", "/admin/banners", {"titulo": "Cyber", "enlace": "/categoria/notebooks-gamer", "ubicacion": "hero"}, admin), 201)
expect("banner futuro", call("POST", "/admin/banners", {"titulo": "Navidad", "desde": "2099-12-01T00:00:00", "ubicacion": "promo"}, admin), 201)
expect("vigencia inválida -> 400", call("POST", "/admin/banners", {"titulo": "X", "desde": "2030-01-02T00:00:00", "hasta": "2030-01-01T00:00:00"}, admin), 400)
pub_b = expect("banners públicos", call("GET", "/banners"), 200)
check("sólo el vigente", [x["titulo"] for x in pub_b] == ["Cyber"], pub_b)
expect("editar banner", call("PUT", f"/admin/banners/{b1['id']}", {"titulo": "Cyber Day", "ubicacion": "hero"}, admin), 200)
expect("newsletter email inválido -> 400", call("POST", "/newsletter", {"email": "nope"}), 400)
expect("suscribirse", call("POST", "/newsletter", {"email": "Fan@Test.com"}), 200)
expect("confirmar token falso -> 400", call("POST", "/newsletter/confirmar", {"token": "x"}), 400)
if LOG:
    import re, time
    time.sleep(0.5)
    toks = re.findall(r"newsletter/confirmar\?token=([0-9a-f]{64})", open(LOG).read())
    check("correo de confirmación (log)", bool(toks))
    if toks:
        expect("confirmar suscripción", call("POST", "/newsletter/confirmar", {"token": toks[-1]}), 200)
        subs = expect("admin lista confirmados", call("GET", "/admin/newsletter?confirmados=true", token=admin), 200)
        check("1 confirmado (email normalizado)", subs["total"] == 1 and subs["items"][0]["email"] == "fan@test.com", subs)
        req = urllib.request.Request(BASE + "/admin/newsletter.csv", headers={"Authorization": f"Bearer {admin}"})
        csv = urllib.request.urlopen(req).read().decode()
        check("CSV exportable", "fan@test.com" in csv and csv.startswith("email,"), csv)
        expect("darse de baja", call("POST", "/newsletter/baja", {"token": toks[-1]}), 200)

print("== Subida de imágenes")
import struct, zlib
def png(w, h):
    """PNG RGB válido generado sin librerías externas."""
    rows = b"".join(b"\x00" + bytes(c for x in range(w) for c in ((x * 7) % 256, (y * 5) % 256, 120)) for y in range(h))
    def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
def upload(data, filename, token):
    boundary = "----jarstoretest"
    body = (f"--{boundary}\r\nContent-Disposition: form-data; name=\"nombre\"\r\n\r\nNotebook ROG frontal\r\n"
            f"--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: image/png\r\n\r\n").encode() + data + f"\r\n--{boundary}--\r\n".encode()
    req = urllib.request.Request(BASE + "/uploads", data=body, method="POST", headers={"Content-Type": f"multipart/form-data; boundary={boundary}", "Authorization": f"Bearer {token}"})
    try:
        with urllib.request.urlopen(req) as r: return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e: return e.code, json.loads(e.read() or b"{}")
expect("buyer no sube -> 403", upload(png(10, 10), "a.png", buyer), 403)
expect("archivo no-imagen -> 400", upload(b"hola", "a.png", seller), 400)
up = expect("subir PNG 1200x900", upload(png(1200, 900), "foto.png", seller), 201)
check("4 tamaños WebP", [v["w"] for v in up["variantes"]] == [320, 640, 1024, 1200], up["variantes"])
check("nombre descriptivo", "notebook-rog-frontal" in up["url"] and up["url"].endswith(".webp"), up["url"])
with urllib.request.urlopen(root + up["variantes"][0]["url"]) as r:
    data = r.read()
    check("se sirve como WebP con caché 1 año", data[8:12] == b"WEBP" and "immutable" in r.headers.get("Cache-Control", ""), r.headers.get("Cache-Control"))
img = expect("asociar imagen subida al producto", call("POST", f"/productos/{gp['id']}/imagenes", {"image_url": up["url"], "variantes": up["variantes"], "is_main": True, "alt_text": "Notebook ROG"}, seller), 201)
check("imagen guarda variantes", len(img["variantes"]) == 4, img)
expect("borrar imagen", call("DELETE", f"/imagenes/{img['id']}", token=seller), 204)
try:
    urllib.request.urlopen(root + up["variantes"][0]["url"]); check("archivo eliminado del disco", False, "sigue existiendo")
except urllib.error.HTTPError as e:
    check("archivo eliminado del disco", e.code == 404, e.code)

if MP_MOCK:
    print("== Mercado Pago (servidor simulado)")
    import hmac, hashlib, http.client
    expect("pagos: MP (Chile) + Pago Móvil (Venezuela)", call("PUT", "/admin/config/pagos", {"metodos": [
        {"id": "mercadopago", "nombre": "Mercado Pago", "tipo": "mercadopago", "paises": ["CL"]},
        {"id": "pago-movil", "nombre": "Pago Móvil", "tipo": "manual", "paises": ["VE"], "instrucciones": "Banco X"},
        {"id": "transferencia", "nombre": "Transferencia bancaria", "instrucciones": "Banco Y"}]}, admin), 200)
    expect("dos MP -> 400", call("PUT", "/admin/config/pagos", {"metodos": [{"id": "a", "nombre": "A", "tipo": "mercadopago"}, {"id": "b", "nombre": "B", "tipo": "mercadopago"}]}, admin), 400)
    adm_cfg = call("GET", "/admin/config", token=admin)[1]
    check("admin ve MP configurado", adm_cfg["mercadopago_configurado"] is True, adm_cfg.get("mercadopago_configurado"))
    pub = call("GET", "/config")[1]
    check("MP público (sin instrucciones)", any(m["tipo"] == "mercadopago" and m.get("instrucciones") is None for m in pub["pagos"]["metodos"]), pub["pagos"])
    _, b3 = new_user("mpbuyer")
    call("POST", "/carrito", {"producto_id": cable["id"], "cantidad": 1}, b3)
    expect("Pago Móvil no disponible en Chile -> 400", call("POST", "/ordenes", {**BUYER, "direccion_envio": "X", "metodo_envio": "retiro", "metodo_pago": "pago-movil"}, b3), 400)
    om = expect("pedido con Mercado Pago", call("POST", "/ordenes", {**BUYER, "direccion_envio": "X", "metodo_envio": "retiro", "metodo_pago": "mercadopago"}, b3), 201)
    check("orden marcada para pago online", om["pago_proveedor"] == "mercadopago" and om["estatus"] == "pendiente", om)
    expect("otro usuario no paga mi orden -> 404", call("POST", f"/ordenes/{om['id']}/pagar", {}, buyer2), 404)
    pay = expect("crear preferencia", call("POST", f"/ordenes/{om['id']}/pagar", {}, b3), 200)
    check("init_point devuelto", pay["init_point"].startswith("https://mp.test/checkout"), pay)
    pref = json.loads(urllib.request.urlopen(MP_MOCK + "/__prefs").read())[-1]
    check("preferencia con total, CLP y referencia", pref["items"][0]["unit_price"] == om["total"] and pref["items"][0]["currency_id"] == "CLP" and pref["external_reference"] == om["id"], pref)
    check("back_urls al pedido y webhook", pref["back_urls"]["success"].endswith(f"/pedido/{om['id']}?pago=aprobado") and pref["notification_url"].endswith("/api/v2/pagos/mercadopago/webhook"), pref)
    def mp_set(**p): urllib.request.urlopen(urllib.request.Request(MP_MOCK + "/__set", data=json.dumps(p).encode(), method="POST", headers={"Content-Type": "application/json"}))
    def webhook(pid, secret=MP_SECRET, req_id="req-1"):
        ts = "1700000000"
        sig = hmac.new(secret.encode(), f"id:{pid};request-id:{req_id};ts:{ts};".encode(), hashlib.sha256).hexdigest()
        c = http.client.HTTPConnection("127.0.0.1", int(root.rsplit(":", 1)[1]))
        c.request("POST", f"/api/v2/pagos/mercadopago/webhook?data.id={pid}&type=payment", body=json.dumps({"type": "payment", "data": {"id": pid}}),
                  headers={"Content-Type": "application/json", "x-signature": f"ts={ts},v1={sig}", "x-request-id": req_id})
        return c.getresponse().status
    mp_set(id=555001, status="pending", external_reference=om["id"], transaction_amount=om["total"], currency_id="CLP", payment_method_id="webpay")
    check("webhook pendiente -> 200", webhook("555001") == 200)
    check("orden sigue pendiente", call("GET", f"/ordenes/{om['id']}", token=b3)[1]["estatus"] == "pendiente")
    check("webhook con firma inválida -> 401", webhook("555001", secret="otro") == 401)
    mp_set(id=555001, status="approved", external_reference=om["id"], transaction_amount=om["total"], currency_id="CLP", payment_method_id="webpay")
    check("webhook aprobado -> 200", webhook("555001") == 200)
    check("webhook repetido es idempotente", webhook("555001") == 200)
    od = call("GET", f"/ordenes/{om['id']}", token=b3)[1]
    check("orden pagada, un solo pago registrado", od["estatus"] == "pagado" and len(od["pagos"]) == 1 and od["pagos"][0]["estatus"] == "completado", od["pagos"])
    expect("ya pagada: no se puede volver a pagar -> 409", call("POST", f"/ordenes/{om['id']}/pagar", {}, b3), 409)
    call("POST", "/carrito", {"producto_id": cable["id"], "cantidad": 1}, b3)
    om2 = call("POST", "/ordenes", {**BUYER, "direccion_envio": "X", "metodo_envio": "retiro", "metodo_pago": "mercadopago"}, b3)[1]
    mp_set(id=555002, status="approved", external_reference=om["id"], transaction_amount=1, currency_id="CLP")
    expect("verificar pago de otra orden -> 400", call("POST", f"/ordenes/{om2['id']}/pago/verificar", {"payment_id": "555002"}, b3), 400)
    mp_set(id=555003, status="approved", external_reference=om2["id"], transaction_amount=om2["total"] - 1, currency_id="CLP")
    v = expect("verificar al volver (monto menor)", call("POST", f"/ordenes/{om2['id']}/pago/verificar", {"payment_id": "555003"}, b3), 200)
    check("monto insuficiente: sigue pendiente", v["estatus"] == "pendiente", v["estatus"])

print("== Programa de puntos")
import threading
expect("reglas que regalan >100% -> 400", call("PUT", "/admin/config/puntos", {"activo": True, "nombre": "Puntos", "monto_base": 1000, "puntos_por_monto": 1, "valor_punto": 2000, "minimo_canje": 0, "maximo_porcentaje": 50}, admin), 400)
expect("reglas: 1 punto por $1.000, punto = $10, mínimo 100, máx 50%", call("PUT", "/admin/config/puntos", {"activo": True, "nombre": "Puntos JarStore", "monto_base": 1000, "puntos_por_monto": 1, "valor_punto": 10, "minimo_canje": 100, "maximo_porcentaje": 50}, admin), 200)
check("reglas públicas en /config", call("GET", "/config")[1]["puntos"]["valor_punto"] == 10)
pu, pt = new_user("puntos")
caro = expect("producto $250.990", call("POST", "/productos", {"nombre": "Tablet 11", "precio": 250990, "stock": 10}, seller), 201)
r = expect("saldo inicial", call("GET", "/puntos", token=pt), 200)
check("saldo 0", r["saldo"] == 0 and r["por_acreditar"] == 0, r)
o1 = expect("compra sin puntos", call("POST", "/ordenes", {**BUYER, "items": [{"producto_id": caro["id"], "cantidad": 1}], "direccion_envio": "X", "metodo_envio": "estandar", "metodo_pago": "transferencia"}, pt), 201)
r = call("GET", "/puntos", token=pt)[1]
check("pendiente: 250 por acreditar, saldo 0 (envío no suma)", r["saldo"] == 0 and r["por_acreditar"] == 250, r)
expect("admin registra el pago", call("POST", f"/admin/ordenes/{o1['id']}/pagos", {"monto": o1["total"], "estatus": "completado"}, admin), 201)
r = call("GET", "/puntos", token=pt)[1]
check("al pagar se acreditan 250 puntos ($2.500)", r["saldo"] == 250 and r["valor_saldo"] == 2500 and r["movimientos"]["items"][0]["tipo"] == "ganados", r)
expect("pago repetido no duplica puntos", call("POST", f"/admin/ordenes/{o1['id']}/pagos", {"monto": 1, "estatus": "completado"}, admin), 201)
check("saldo sigue en 250", call("GET", "/puntos", token=pt)[1]["saldo"] == 250)
q = expect("cotizar con 200 puntos", call("POST", "/ordenes/cotizar", {"items": [{"producto_id": cable["id"], "cantidad": 1}], "pais": "CL", "metodo_envio": "retiro", "puntos": 200}, pt), 200)
check("tope 50% de $9.990 = 499 puntos -> 200 ok, descuento $2.000", q["puntos"]["max_usables"] == 250 and q["puntos"]["descuento"] == 2000 and q["total"] == 9990 - 2000, q["puntos"])
check("cotización informa puntos a ganar", q["puntos"]["ganara"] == 7, q["puntos"])
q = expect("cotizar con exceso de puntos", call("POST", "/ordenes/cotizar", {"items": [{"producto_id": cable["id"], "cantidad": 1}], "pais": "CL", "metodo_envio": "retiro", "puntos": 900}, pt), 200)
check("exceso: no aplica y explica", q["puntos"]["usados"] == 0 and q["puntos"]["error"], q["puntos"])
expect("checkout con más puntos de los permitidos -> 400", call("POST", "/ordenes", {**BUYER, "items": [{"producto_id": cable["id"], "cantidad": 1}], "direccion_envio": "X", "metodo_envio": "retiro", "metodo_pago": "transferencia", "puntos": 900}, pt), 400)
op = expect("checkout canjeando 200 puntos", call("POST", "/ordenes", {**BUYER, "items": [{"producto_id": cable["id"], "cantidad": 1}], "direccion_envio": "X", "metodo_envio": "retiro", "metodo_pago": "transferencia", "puntos": 200}, pt), 201)
check("orden con descuento por puntos", op["puntos_usados"] == 200 and op["descuento_puntos"] == 2000 and op["total"] == 7990, op)
check("saldo descontado al instante (50)", call("GET", "/puntos", token=pt)[1]["saldo"] == 50)
expect("bajo el mínimo de canje -> 400", call("POST", "/ordenes", {**BUYER, "items": [{"producto_id": cable["id"], "cantidad": 1}], "direccion_envio": "X", "metodo_envio": "retiro", "metodo_pago": "transferencia", "puntos": 50}, pt), 400)
expect("cancelar el pedido con puntos", call("POST", f"/ordenes/{op['id']}/cancelar", token=pt), 200)
check("puntos devueltos (250)", call("GET", "/puntos", token=pt)[1]["saldo"] == 250)
expect("cancelar de nuevo -> 409", call("POST", f"/ordenes/{op['id']}/cancelar", token=pt), 409)
check("sin devoluciones duplicadas", call("GET", "/puntos", token=pt)[1]["saldo"] == 250)
expect("admin cancela el pedido pagado", call("PUT", f"/admin/ordenes/{o1['id']}/estatus", {"estatus": "cancelado"}, admin), 200)
r = call("GET", "/puntos", token=pt)[1]
check("se revierten los 250 ganados", r["saldo"] == 0 and r["movimientos"]["items"][0]["tipo"] == "revertidos", r["movimientos"]["items"][:2])
uid = call("GET", "/users/me", token=pt)[1]["id"]
expect("ajuste admin sin motivo -> 400", call("POST", f"/admin/users/{uid}/puntos", {"puntos": 300, "motivo": ""}, admin), 400)
expect("ajuste que deja saldo negativo -> 400", call("POST", f"/admin/users/{uid}/puntos", {"puntos": -5, "motivo": "x"}, admin), 400)
a = expect("ajuste admin +300", call("POST", f"/admin/users/{uid}/puntos", {"puntos": 300, "motivo": "Compensación por demora"}, admin), 200)
check("saldo 300 tras ajuste", a["saldo"] == 300, a)
expect("cliente no hace ajustes -> 403", call("POST", f"/admin/users/{uid}/puntos", {"puntos": 1, "motivo": "x"}, pt), 403)
# Doble gasto: dos compras simultáneas usando el mismo saldo
codes = []
def gastar():
    codes.append(call("POST", "/ordenes", {**BUYER, "items": [{"producto_id": caro["id"], "cantidad": 1}], "direccion_envio": "X", "metodo_envio": "retiro", "metodo_pago": "transferencia", "puntos": 300}, pt)[0])
ts = [threading.Thread(target=gastar) for _ in range(6)]
[t.start() for t in ts]; [t.join() for t in ts]
check("sólo una compra puede usar los mismos 300 puntos", codes.count(201) == 1 and codes.count(400) == 5, codes)
check("saldo final 0, nunca negativo", call("GET", "/puntos", token=pt)[1]["saldo"] == 0)
expect("desactivar programa", call("PUT", "/admin/config/puntos", {"activo": False, "nombre": "Puntos JarStore", "monto_base": 1000, "puntos_por_monto": 1, "valor_punto": 10, "minimo_canje": 100, "maximo_porcentaje": 50}, admin), 200)
call("POST", f"/admin/users/{uid}/puntos", {"puntos": 500, "motivo": "test"}, admin)
expect("programa inactivo: no se canjea -> 400", call("POST", "/ordenes", {**BUYER, "items": [{"producto_id": cable["id"], "cantidad": 1}], "direccion_envio": "X", "metodo_envio": "retiro", "metodo_pago": "transferencia", "puntos": 100}, pt), 400)
call("PUT", "/admin/config/puntos", {"activo": True, "nombre": "Puntos JarStore", "monto_base": 1000, "puntos_por_monto": 1, "valor_punto": 10, "minimo_canje": 100, "maximo_porcentaje": 50}, admin)

print("== Facturación (boleta/factura) y documento")
# Chile: boleta por defecto
_, bfac = new_user("facbuyer")
call("POST", "/carrito", {"producto_id": cable["id"], "cantidad": 1}, bfac)
ob = expect("checkout CL sin tipo -> boleta por defecto", call("POST", "/ordenes", {**BUYER, "direccion_envio": "Calle 1", "metodo_envio": "retiro", "metodo_pago": "transferencia"}, bfac), 201)
check("tipo_documento = boleta", ob["tipo_documento"] == "boleta" and ob["factura_rut"] is None, ob.get("tipo_documento"))
# Factura con RUT inválido
call("POST", "/carrito", {"producto_id": cable["id"], "cantidad": 1}, bfac)
expect("factura con RUT inválido -> 400", call("POST", "/ordenes", {**BUYER, "direccion_envio": "x", "metodo_envio": "retiro", "metodo_pago": "transferencia", "tipo_documento": "factura", "factura_rut": "12345678-9", "factura_razon_social": "ACME", "factura_giro": "Ventas", "factura_direccion": "Av 1"}, bfac), 400)
# Factura sin razón social
expect("factura sin razón social -> 400", call("POST", "/ordenes", {**BUYER, "direccion_envio": "x", "metodo_envio": "retiro", "metodo_pago": "transferencia", "tipo_documento": "factura", "factura_rut": "12.345.678-5", "factura_giro": "Ventas", "factura_direccion": "Av 1"}, bfac), 400)
# Factura completa (RUT con puntos se normaliza)
of = expect("factura completa", call("POST", "/ordenes", {**BUYER, "direccion_envio": "x", "metodo_envio": "retiro", "metodo_pago": "transferencia", "tipo_documento": "factura", "factura_rut": "12.345.678-5", "factura_razon_social": "ACME SpA", "factura_giro": "Comercio", "factura_direccion": "Av Siempre Viva 123"}, bfac), 201)
check("factura guarda datos y normaliza RUT", of["tipo_documento"] == "factura" and of["factura_rut"] == "12345678-5" and of["factura_razon_social"] == "ACME SpA", {k: of.get(k) for k in ("tipo_documento","factura_rut","factura_razon_social","factura_giro")})
# tipo inválido
call("POST", "/carrito", {"producto_id": cable["id"], "cantidad": 1}, bfac)
expect("tipo_documento inválido -> 400", call("POST", "/ordenes", {**BUYER, "direccion_envio": "x", "metodo_envio": "retiro", "metodo_pago": "transferencia", "tipo_documento": "ticket"}, bfac), 400)
# Venezuela: sin documento tributario (requiere país VE con método; VE no tiene métodos -> a coordinar)
_, bve = new_user("vebuyer")
VE = {"pais": "VE", "metodo_pago": "transferencia", "nombre_comprador": "Juan", "email_comprador": "j@t.com"}
call("POST", "/carrito", {"producto_id": cable["id"], "cantidad": 1}, bve)
ove = expect("checkout VE (sin documento tributario)", call("POST", "/ordenes", {**VE, "direccion_envio": "Caracas 1", "tipo_documento": "factura", "factura_rut": "x"}, bve), 201)
check("VE ignora documento tributario", ove["tipo_documento"] is None, ove.get("tipo_documento"))

print("== Subida del documento (admin)")
import urllib.request, urllib.error
def upload_doc(order_id, content, filename, ctype, token):
    bd = "----doc"
    body = (f"--{bd}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {ctype}\r\n\r\n").encode() + content + f"\r\n--{bd}--\r\n".encode()
    r = urllib.request.Request(BASE + f"/admin/ordenes/{order_id}/documento", data=body, method="POST", headers={"Content-Type": f"multipart/form-data; boundary={bd}", "Authorization": "Bearer " + token})
    try:
        with urllib.request.urlopen(r, timeout=15) as x: return x.status, json.loads(x.read())
    except urllib.error.HTTPError as e:
        try: return e.code, json.loads(e.read())
        except Exception: return e.code, "?"
pdf = b"%PDF-1.4\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF"
expect("buyer no sube documento -> 403", upload_doc(of["id"], pdf, "f.pdf", "application/pdf", bfac), 403)
expect("documento de texto plano -> 400", upload_doc(of["id"], b"no soy pdf ni imagen", "f.pdf", "application/pdf", admin), 400)
od = expect("admin sube factura PDF", upload_doc(of["id"], pdf, "factura.pdf", "application/pdf", admin), 200)
check("orden queda con documento_url .pdf", od.get("documento_url", "").endswith(".pdf") and od.get("documento_subido_at"), od.get("documento_url"))
# el cliente ve y puede descargar su documento
oc = call("GET", f"/ordenes/{of['id']}", token=bfac)[1]
check("cliente ve el documento en su pedido", oc["documento_url"] == od["documento_url"], oc.get("documento_url"))
# el PDF se sirve con content-type correcto
import http.client
c = http.client.HTTPConnection("127.0.0.1", int(root.rsplit(":", 1)[1])); c.request("GET", od["documento_url"]); rr = c.getresponse(); data = rr.read()
check("PDF servido con content-type application/pdf", rr.status == 200 and "application/pdf" in rr.getheader("Content-Type", "") and data.startswith(b"%PDF"), rr.getheader("Content-Type"))

print("== Correos de estado (modo desarrollo: al log)")
_, est = new_user("estadobuyer")
estprod = call("POST", "/productos", {"nombre": "Producto estados", "precio": 15000, "stock": 100}, seller)[1]
oe = expect("pedido para seguir estados", call("POST", "/ordenes", {**BUYER, "items": [{"producto_id": estprod["id"], "cantidad": 1}], "direccion_envio": "x", "metodo_envio": "retiro", "metodo_pago": "transferencia"}, est), 201)
short = oe["id"][:8].upper()
expect("admin registra pago (dispara 'pagado')", call("POST", f"/admin/ordenes/{oe['id']}/pagos", {"monto": oe["total"], "estatus": "completado"}, admin), 201)
expect("admin marca enviado", call("PUT", f"/admin/ordenes/{oe['id']}/estatus", {"estatus": "enviado"}, admin), 200)
expect("admin marca entregado", call("PUT", f"/admin/ordenes/{oe['id']}/estatus", {"estatus": "entregado"}, admin), 200)
if LOG:
    import time; time.sleep(0.6)
    logtxt = open(LOG).read()
    check("correo 'Pago confirmado' encolado", f"Pago confirmado — Pedido #{short}" in logtxt, short)
    check("correo 'va en camino' encolado", f"Tu pedido va en camino — #{short}" in logtxt)
    check("correo 'entregado' encolado", f"Pedido entregado — #{short}" in logtxt)
    # aviso de pedido nuevo a la tienda: sólo si hay email de tienda; configurémoslo
tienda_cfg = call("GET", "/config")[1]["tienda"]
call("PUT", "/admin/config/tienda", {**tienda_cfg, "email": "ventas@jarstore.cl"}, admin)
_, est2 = new_user("avisobuyer")
on = call("POST", "/ordenes", {**BUYER, "items": [{"producto_id": estprod["id"], "cantidad": 1}], "direccion_envio": "x", "metodo_envio": "retiro", "metodo_pago": "transferencia"}, est2)[1]
if LOG:
    import time; time.sleep(0.5)
    check("aviso de pedido nuevo a la tienda", f"Nuevo pedido #{on['id'][:8].upper()}" in open(LOG).read())

print("== Panel de ventas (métricas)")
expect("buyer no ve métricas -> 403", call("GET", "/admin/metricas", token=buyer), 403)
m = expect("admin ve métricas", call("GET", "/admin/metricas", token=admin), 200)
check("resumen con ingresos y pedidos > 0", m["resumen"]["ingresos"] > 0 and m["resumen"]["pedidos"] > 0, m["resumen"])
check("ticket promedio = ingresos/pedidos", abs(m["resumen"]["ticket_promedio"] - round(m["resumen"]["ingresos"]/m["resumen"]["pedidos"])) <= 1, m["resumen"])
check("hay desglose por estado", any(e["estatus"] in ("pagado","enviado","entregado") for e in m["por_estado"]), m["por_estado"])
check("hay serie por día", len(m["por_dia"]) >= 1 and "fecha" in m["por_dia"][0], m["por_dia"][:1])
check("hay top de productos con ingresos", len(m["top_productos"]) >= 1 and m["top_productos"][0]["ingresos"] > 0, m["top_productos"][:1])
check("pendientes contabilizados aparte", m["resumen"]["pendientes"] >= 0)
import urllib.parse
r = expect("rango de fechas sin ventas", call("GET", "/admin/metricas?desde=2020-01-01&hasta=2020-01-31", token=admin), 200)
check("rango sin ventas: ingresos 0", r["resumen"]["ingresos"] == 0, r["resumen"]["ingresos"])
expect("rango invertido -> 400", call("GET", "/admin/metricas?desde=2030-01-01&hasta=2020-01-01", token=admin), 400)

print("== Correo (SMTP configurable desde el panel)")
expect("SMTP activo sin host -> 400", call("PUT", "/admin/config/smtp", {"activo": True, "host": "", "port": 587, "security": "starttls", "from_email": "x@y.cl"}, admin), 400)
expect("seguridad inválida -> 400", call("PUT", "/admin/config/smtp", {"activo": False, "host": "smtp.x.cl", "security": "raro", "from_email": "x@y.cl"}, admin), 400)
expect("guardar SMTP (inactivo) con contraseña", call("PUT", "/admin/config/smtp", {"activo": False, "host": "smtp.x.cl", "port": 587, "security": "starttls", "usuario": "user", "password": "secreto123", "from_nombre": "JarStore", "from_email": "no-reply@jarstore.cl"}, admin), 200)
cfg = call("GET", "/admin/config", token=admin)[1]["config"]
smtp = next(e["valor"] for e in cfg if e["clave"] == "smtp")
check("contraseña SMTP NO se expone", smtp["password"] == "" and smtp.get("password_set") is True, smtp)
# editar sin enviar contraseña: debe conservarla
expect("editar SMTP sin reenviar contraseña", call("PUT", "/admin/config/smtp", {"activo": False, "host": "smtp.x.cl", "port": 465, "security": "tls", "usuario": "user", "password": "", "from_nombre": "Tienda", "from_email": "no-reply@jarstore.cl"}, admin), 200)
cfg = call("GET", "/admin/config", token=admin)[1]["config"]
check("contraseña conservada tras editar", next(e["valor"]["password_set"] for e in cfg if e["clave"]=="smtp") is True)
# prueba SMTP sin configuración activa -> 400 'no hay SMTP'
r = call("POST", "/admin/config/smtp/test", {"email": "test@test.cl"}, admin)
check("prueba sin SMTP activo informa que falta configurar", r[0] == 400, r)
# activar con host que no responde -> la prueba reporta error de conexión (no cuelga)
call("PUT", "/admin/config/smtp", {"activo": True, "host": "127.0.0.1", "port": 2, "security": "none", "from_email": "no-reply@jarstore.cl", "password": "x"}, admin)
r = call("POST", "/admin/config/smtp/test", {"email": "test@test.cl"}, admin)
check("prueba con host caído -> 400 con error de envío", r[0] == 400 and ("No se pudo enviar" in str(r[1]) or "enviar" in str(r[1])), r)
# volver a inactivo para no afectar otros correos del test
call("PUT", "/admin/config/smtp", {"activo": False, "host": "", "from_email": "no-reply@jarstore.cl"}, admin)

print("== SEO en el servidor")
import http.client
def raw(path):
    c = http.client.HTTPConnection("127.0.0.1", int(root.rsplit(":", 1)[1]))
    c.request("GET", path); r = c.getresponse(); return r.status, dict(r.getheaders()), r.read().decode()
st, h, html = raw("/")
if "<title>" in html:
    check("home: WebSite + SearchAction + Organization", '"SearchAction"' in html and '"Organization"' in html and "/buscar?q={search_term_string}" in html, html[:300])
    st, h, html = raw("/producto/notebook-rog-15")
    check("producto: 200 con title y canonical", st == 200 and "<title>Notebook Gamer" in html and 'rel="canonical" href="https://tienda.test/producto/notebook-rog-15"' in html, html[:400])
    check("producto: JSON-LD Product/Offer en CLP", '"@type":"Product"' in html and '"priceCurrency":"CLP"' in html and '"price":"999990"' in html, html)
    check("producto: BreadcrumbList", '"BreadcrumbList"' in html and "notebooks-gamer" in html)
    st, h, _ = raw("/producto/notebook-gamer-nandu-15")
    check("slug antiguo -> 301", st == 301 and h.get("location") == "/producto/notebook-rog-15", (st, h.get("location")))
    st, h, _ = raw(f"/producto/{gp['id']}")
    check("URL con uuid -> 301 al slug", st == 301 and h.get("location") == "/producto/notebook-rog-15", (st, h.get("location")))
    st, _, html = raw("/producto/no-existe")
    check("producto inexistente -> 404 noindex", st == 404 and "noindex" in html, st)
    st, _, html = raw("/categoria/notebooks-gamer?page=2")
    check("categoría paginada: canonical con page", 'href="https://tienda.test/categoria/notebooks-gamer?page=2"' in html and "noindex" not in html, html[:500])
    st, _, html = raw("/categoria/notebooks-gamer?marca=Asus")
    check("categoría filtrada: noindex", "noindex" in html)
    st, _, html = raw("/cuenta/pedidos")
    check("área privada: noindex", "noindex" in html)
    st, h, _ = raw("/assets/app-abc123.js")
    check("assets con caché immutable", st == 200 and "immutable" in h.get("cache-control", ""), h.get("cache-control"))
    st, _, xml = raw("/sitemap.xml")
    check("sitemap con producto, categoría, marca y página", st == 200 and "/producto/notebook-rog-15" in xml and "/categoria/notebooks-gamer" in xml and "/marca/asus-rog" in xml and "/p/devoluciones" in xml, xml[:300])
    check("sitemap sin productos ocultos", "/producto/oculto" not in xml)
    st, _, txt = raw("/robots.txt")
    check("robots.txt con Sitemap", "Sitemap: https://tienda.test/sitemap.xml" in txt, txt)
    st, _, body = raw("/api/no-existe")
    check("API desconocida sigue en JSON", st == 404 and '"NOT_FOUND"' in body, body)
else:
    print("  (frontend no servido: se omiten pruebas de HTML)")

print("== Concurrencia: 10 compras simultáneas de un producto con stock 1")
import threading
last = expect("producto con stock 1", call("POST", "/productos", {"nombre": "Último", "precio": 100, "stock": 1}, seller), 201)
buyers = [new_user(f"race{i}")[1] for i in range(10)]
results = []
def buy(tok):
    results.append(call("POST", "/ordenes", {**BUYER, "items": [{"producto_id": last["id"], "cantidad": 1}], "direccion_envio": "X", "metodo_envio": "retiro", "metodo_pago": "transferencia"}, tok)[0])
threads = [threading.Thread(target=buy, args=(t,)) for t in buyers]
[t.start() for t in threads]; [t.join() for t in threads]
check("exactamente 1 compra exitosa, sin sobreventa", results.count(201) == 1 and results.count(400) == 9, results)
check("stock final 0", call("GET", f"/productos/{last['id']}")[1]["stock"] == 0)

# ----------------------------------------------------------------------------
# Envíos avanzados: courier (Blue Express) para Chile y por peso/tamaño para
# Venezuela. Va al final porque reconfigura el envío global (modo courier/peso),
# lo que cambia los métodos disponibles para el resto del catálogo.
print("== Envíos: peso de productos, courier (Blue Express) y por peso/tamaño")
heavy = expect("producto con peso y dimensiones", call("POST", "/productos",
        {"nombre": "Refrigerador", "precio": 299990, "stock": 5,
         "peso_gramos": 45000, "largo_cm": 70, "ancho_cm": 70, "alto_cm": 180}, seller), 201)
check("peso y dimensiones se guardan", heavy["peso_gramos"] == 45000 and heavy["largo_cm"] == 70 and heavy["alto_cm"] == 180, heavy)
check("peso negativo -> 400", call("POST", "/productos", {"nombre": "Malo", "precio": 1, "stock": 1, "peso_gramos": -5}, seller)[0] == 400)

expect("configurar courier (CL) y por peso (VE)", call("PUT", "/admin/config/envio", {"paises": [
    {"codigo": "CL", "nombre": "Chile", "modo": "courier",
     "metodos": [{"id": "retiro", "nombre": "Retiro en tienda", "precio": 0}],
     "courier": {"origen": "STGO", "recargo": 500, "gratis_desde": 100000000}},
    {"codigo": "VE", "nombre": "Venezuela", "modo": "peso",
     "peso": {"nombre": "Envío nacional", "base": 5000, "precio_por_kg": 800, "factor_volumetrico": 4000, "plazo": "3 a 5 días"}},
]}, admin), 200)
check("courier sin comuna de origen -> 400", call("PUT", "/admin/config/envio", {"paises": [
    {"codigo": "CL", "nombre": "Chile", "modo": "courier", "courier": {"origen": ""}}]}, admin)[0] == 400)

it = [{"producto_id": heavy["id"], "cantidad": 1}]
q = expect("cotizar CL con comuna (courier)", call("POST", "/ordenes/cotizar", {"pais": "CL", "comuna": "PUDA", "items": it}, buyer2), 200)
bx = [m for m in q["metodos_envio"] if m["id"].startswith("bx-")]
check("Blue Express devuelve opciones cotizadas", len(bx) >= 1 and all(m["descripcion"] == "Cotizado con Blue Express" for m in bx), q["metodos_envio"])
check("recargo de la tienda incluido", all(m["costo"] >= 500 for m in bx), bx)
q2 = expect("cotizar CL sin comuna (fallback manual)", call("POST", "/ordenes/cotizar", {"pais": "CL", "items": it}, buyer2), 200)
check("sin comuna cae a métodos manuales", any(m["id"] == "retiro" for m in q2["metodos_envio"]) and not q2["envio_a_coordinar"], q2)
q3 = expect("cotizar VE por peso/tamaño", call("POST", "/ordenes/cotizar", {"pais": "VE", "items": it}, buyer2), 200)
pm = [m for m in q3["metodos_envio"] if m["id"] == "peso"]
check("VE cobra por peso volumétrico", len(pm) == 1 and pm[0]["nombre"] == "Envío nacional" and pm[0]["costo"] > 5000 and "kg" in (pm[0]["descripcion"] or ""), q3)
sel, costo = bx[0]["id"], bx[0]["costo"]
ovc = expect("checkout CL con courier", call("POST", "/ordenes", {**BUYER, "items": it, "direccion_envio": "Av 1",
        "metodo_envio": sel, "comuna": "PUDA", "metodo_pago": "transferencia"}, buyer2), 201)
check("guarda comuna de destino y costo del courier", ovc["comuna_destino"] == "PUDA" and ovc["costo_envio"] == costo, ovc)

print("== Conversión: avísame cuando vuelva, carrito abandonado, stock bajo")
ag = expect("producto agotado", call("POST", "/productos", {"nombre": "Agotado X", "precio": 9990, "stock": 0}, seller), 201)
expect("avísame cuando vuelva", call("POST", f"/productos/{ag['id']}/avisar-stock", {"email": "espera@test.com"}), 200)
check("no duplica el aviso pendiente", call("POST", f"/productos/{ag['id']}/avisar-stock", {"email": "espera@test.com"})[0] == 200)
disp = expect("producto disponible", call("POST", "/productos", {"nombre": "Disp X", "precio": 100, "stock": 3}, seller), 201)
expect("avisar producto disponible -> 400", call("POST", f"/productos/{disp['id']}/avisar-stock", {"email": "x@test.com"}), 400)
expect("avisar producto inexistente -> 404", call("POST", f"/productos/{uuid.uuid4()}/avisar-stock", {"email": "x@test.com"}), 404)
r = expect("procesar sin reposición", call("POST", "/admin/notificaciones/procesar", {}, admin), 200)
check("sin avisos de stock todavía", r["avisos_stock"] == 0, r)
expect("reponer stock", call("PUT", f"/productos/{ag['id']}", {"stock": 5}, admin), 200)
r = expect("procesar con reposición", call("POST", "/admin/notificaciones/procesar", {}, admin), 200)
check("envía el aviso de stock", r["avisos_stock"] >= 1, r)
check("no repite el aviso ya enviado", call("POST", "/admin/notificaciones/procesar", {}, admin)[1]["avisos_stock"] == 0)
cu, ctok = new_user("carrito")
call("POST", "/carrito", {"producto_id": disp["id"], "cantidad": 1}, ctok)
check("carrito reciente no se avisa", call("POST", "/admin/notificaciones/procesar", {}, admin)[1]["recordatorios_carrito"] == 0)
psql(f"UPDATE carrito SET created_at = NOW() - INTERVAL '3 hours' WHERE user_id = (SELECT id FROM users WHERE username='{cu}')")
r = expect("procesar carrito antiguo", call("POST", "/admin/notificaciones/procesar", {}, admin), 200)
check("envía el recordatorio de carrito", r["recordatorios_carrito"] >= 1, r)
check("no repite el recordatorio de carrito", call("POST", "/admin/notificaciones/procesar", {}, admin)[1]["recordatorios_carrito"] == 0)
low = expect("stock bajo (admin)", call("GET", "/admin/stock-bajo?umbral=5", token=admin), 200)
check("stock bajo lista el producto con poco stock", any(i["nombre"] == "Disp X" for i in low), low)
expect("stock bajo requiere admin -> 403", call("GET", "/admin/stock-bajo", token=buyer), 403)

print(f"\n{passed} ok, {failed} fallidas")
sys.exit(1 if failed else 0)
