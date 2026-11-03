use crate::auth::AuthUser;
use crate::courier::Courier;
use crate::errors::{ApiError, ApiResult, OrNotFound};
use crate::models::carrito::CarritoItem;
use crate::models::tienda::{ModoEnvio, PaisEnvio};
use crate::models::extras::{Cupon, Direccion};
use crate::models::orden::*;
use crate::models::products::{Producto, Variante};
use crate::schema::{carrito, cupones, detalle_orden, direcciones, envios, ordenes, pagos, product_variants, productos};
use crate::services::common::*;
use crate::services::{puntos_services, tienda_services};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

#[derive(Deserialize, Debug)]
pub struct OrderItemRequest {
    pub producto_id: Uuid,
    pub variante_id: Option<Uuid>,
    pub cantidad: i32,
}

/// Crear orden. Si no se envían `items`, se usa el carrito del usuario (y se vacía).
/// Los precios, el descuento y el costo de envío SIEMPRE se calculan en el servidor.
#[derive(Deserialize, Debug)]
pub struct CheckoutRequest {
    pub items: Option<Vec<OrderItemRequest>>,
    /// Usar una dirección guardada del usuario...
    pub direccion_id: Option<Uuid>,
    /// ...o escribir la dirección directamente.
    pub direccion_envio: Option<String>,
    /// Código ISO del país de entrega (CL, VE...). Debe existir en la configuración de envío.
    pub pais: String,
    /// id del método de envío del país (si el país no tiene métodos, el envío queda "a coordinar")
    pub metodo_envio: Option<String>,
    /// Comuna de destino (código del courier), requerida cuando el país cotiza con courier
    pub comuna: Option<String>,
    /// id del método de pago configurado
    pub metodo_pago: Option<String>,
    /// Moneda en que el cliente ve la compra (por defecto la base)
    pub moneda: Option<String>,
    pub cupon: Option<String>,
    /// Puntos a canjear como descuento
    #[serde(default)]
    pub puntos: i64,
    pub nombre_comprador: String,
    pub email_comprador: String,
    pub telefono_comprador: Option<String>,
    /// Sólo Chile: "boleta" (por defecto) o "factura"
    pub tipo_documento: Option<String>,
    /// Datos de facturación (obligatorios si tipo_documento = "factura")
    pub factura_rut: Option<String>,
    pub factura_razon_social: Option<String>,
    pub factura_giro: Option<String>,
    pub factura_direccion: Option<String>,
}

/// Cotización (sin crear la orden): lo que vería el cliente antes de confirmar.
#[derive(Deserialize, Debug)]
pub struct QuoteRequest {
    pub items: Option<Vec<OrderItemRequest>>,
    pub pais: Option<String>,
    pub metodo_envio: Option<String>,
    pub comuna: Option<String>,
    pub cupon: Option<String>,
    #[serde(default)]
    pub puntos: i64,
}

#[derive(Serialize, Debug, Default)]
pub struct PuntosCotizados {
    pub activo: bool,
    pub saldo: i64,
    /// Máximo que se puede usar en este pedido
    pub max_usables: i64,
    pub usados: i64,
    pub descuento: i64,
    pub valor_punto: i64,
    pub minimo_canje: i64,
    /// Puntos que ganará con este pedido al pagarlo
    pub ganara: i64,
    pub error: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct MetodoEnvioCotizado {
    pub id: String,
    pub nombre: String,
    pub costo: i64,
    pub precio_normal: i64,
    pub gratis_desde: Option<i64>,
    pub plazo: Option<String>,
    pub descripcion: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct Cotizacion {
    pub subtotal: i64,
    pub descuento: i64,
    pub cupon: Option<String>,
    pub cupon_error: Option<String>,
    pub costo_envio: i64,
    pub metodo_envio: Option<String>,
    /// true si el país no tiene métodos configurados: el costo se coordina después
    pub envio_a_coordinar: bool,
    pub metodos_envio: Vec<MetodoEnvioCotizado>,
    pub puntos: PuntosCotizados,
    pub total: i64,
}

/// Valida un canje y devuelve el descuento equivalente.
fn puntos_para(conn: &mut PgConnection, user_id: Uuid, pedidos: i64, canjeable: i64) -> ApiResult<(i64, i64, PuntosCotizados)> {
    let cfg = puntos_services::config(conn)?;
    let saldo = puntos_services::saldo(conn, user_id)?;
    let max = cfg.max_usables(saldo, canjeable);
    let mut info = PuntosCotizados { activo: cfg.activo, saldo, max_usables: max, valor_punto: cfg.valor_punto, minimo_canje: cfg.minimo_canje, ..Default::default() };
    if pedidos <= 0 {
        return Ok((0, 0, info));
    }
    if !cfg.activo {
        return Err(ApiError::bad_request("El programa de puntos no está activo"));
    }
    if saldo < cfg.minimo_canje {
        return Err(ApiError::bad_request(format!("Necesitas al menos {} puntos para canjear (tienes {saldo})", cfg.minimo_canje)));
    }
    if pedidos > max {
        return Err(ApiError::bad_request(format!("Puedes usar como máximo {max} puntos en este pedido")));
    }
    let descuento = (pedidos * cfg.valor_punto).min(canjeable);
    info.usados = pedidos;
    info.descuento = descuento;
    Ok((pedidos, descuento, info))
}

const ENVIO_A_COORDINAR: &str = "Despacho a coordinar";
const PAGO_A_COORDINAR: &str = "A coordinar con la tienda";

#[derive(Deserialize)]
pub struct StatusRequest {
    pub estatus: EstatusOrden,
}

#[derive(Deserialize)]
pub struct OrderFilter {
    pub estatus: Option<EstatusOrden>,
    pub user_id: Option<Uuid>,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EstatusPago {
    Pendiente,
    Completado,
    Fallido,
}

impl EstatusPago {
    fn as_str(&self) -> &'static str {
        match self {
            EstatusPago::Pendiente => "pendiente",
            EstatusPago::Completado => "completado",
            EstatusPago::Fallido => "fallido",
        }
    }
}

#[derive(Deserialize)]
pub struct PagoRequest {
    pub monto: i64,
    pub metodo: Option<String>,
    pub estatus: EstatusPago,
    pub referencia_externa: Option<String>,
}

#[derive(Serialize)]
pub struct OrdenDetalle {
    #[serde(flatten)]
    pub orden: Orden,
    pub detalles: Vec<DetalleOrden>,
    pub pagos: Vec<Pago>,
    pub envio: Option<Envio>,
}

// ---------- Helpers ----------

fn get_order_for_update(conn: &mut PgConnection, id: Uuid) -> ApiResult<Orden> {
    ordenes::table.find(id).select(Orden::as_select()).for_update().first(conn).or_not_found("Orden")
}

pub fn detail_of(conn: &mut PgConnection, orden: Orden) -> ApiResult<OrdenDetalle> {
    let detalles = detalle_orden::table
        .filter(detalle_orden::orden_id.eq(orden.id))
        .select(DetalleOrden::as_select())
        .load(conn)?;
    let pagos = pagos::table
        .filter(pagos::orden_id.eq(orden.id))
        .order(pagos::created_at.asc())
        .select(Pago::as_select())
        .load(conn)?;
    let envio = envios::table
        .filter(envios::orden_id.eq(orden.id))
        .select(Envio::as_select())
        .first(conn)
        .optional()?;
    Ok(OrdenDetalle { orden, detalles, pagos, envio })
}

fn overflow() -> ApiError {
    ApiError::bad_request("El monto de la orden es demasiado grande")
}

/// Busca un cupón por código (sin distinguir mayúsculas) bloqueándolo para actualizar.
pub fn find_coupon(conn: &mut PgConnection, codigo: &str, lock: bool) -> ApiResult<Cupon> {
    let codigo = codigo.trim().to_uppercase();
    let q = cupones::table.filter(cupones::codigo.eq(codigo)).select(Cupon::as_select());
    let res = if lock { q.for_update().first(conn) } else { q.first(conn) };
    res.or_not_found("Cupón")
}

// ---------- Checkout ----------

/// (producto, variante) => cantidad
type Lines = BTreeMap<(Uuid, Option<Uuid>), i32>;

struct Priced {
    lines: Lines,
    detalles: Vec<NewDetalleOrden>,
    subtotal: i64,
    /// Peso real total del pedido (gramos). Productos sin peso cuentan como PESO_DEFECTO_G.
    peso_g: i64,
    /// Volumen total del pedido (cm³), para el peso volumétrico. 0 si no hay dimensiones.
    volumen_cm3: i64,
}

/// Peso asumido para un producto que no declara peso (1 kg).
const PESO_DEFECTO_G: i64 = 1000;

fn collect_lines(conn: &mut PgConnection, user_id: Uuid, items: Option<Vec<OrderItemRequest>>) -> ApiResult<(bool, Lines)> {
    let from_cart = items.is_none();
    let raw: Vec<(Uuid, Option<Uuid>, i32)> = match items {
        Some(items) => items.into_iter().map(|i| (i.producto_id, i.variante_id, i.cantidad)).collect(),
        None => carrito::table
            .filter(carrito::user_id.eq(user_id))
            .select(CarritoItem::as_select())
            .load(conn)?
            .into_iter()
            .map(|c| (c.producto_id, c.variante_id, c.cantidad))
            .collect(),
    };
    if raw.is_empty() {
        return Err(ApiError::bad_request(if from_cart { "El carrito está vacío" } else { "La orden no tiene productos" }));
    }
    // Agrupar líneas repetidas (BTreeMap => orden estable, evita deadlocks)
    let mut lines = BTreeMap::new();
    for (pid, vid, qty) in raw {
        positive_qty(qty)?;
        *lines.entry((pid, vid)).or_insert(0) += qty;
    }
    Ok((from_cart, lines))
}

/// Calcula precios y valida stock. Con `lock` bloquea las filas (para el checkout real).
fn price_lines(conn: &mut PgConnection, lines: Lines, lock: bool) -> ApiResult<Priced> {
    let mut product_ids: Vec<Uuid> = lines.keys().map(|(p, _)| *p).collect();
    product_ids.sort();
    product_ids.dedup();
    let variant_ids: Vec<Uuid> = lines.keys().filter_map(|(_, v)| *v).collect();

    let pq = productos::table.filter(productos::id.eq_any(&product_ids)).order(productos::id).select(Producto::as_select());
    let products: Vec<Producto> = if lock { pq.for_update().load(conn)? } else { pq.load(conn)? };
    let products: HashMap<Uuid, Producto> = products.into_iter().map(|p| (p.id, p)).collect();
    let vq = product_variants::table.filter(product_variants::id.eq_any(&variant_ids)).order(product_variants::id).select(Variante::as_select());
    let variants: Vec<Variante> = if lock { vq.for_update().load(conn)? } else { vq.load(conn)? };
    let variants: HashMap<Uuid, Variante> = variants.into_iter().map(|v| (v.id, v)).collect();

    let mut subtotal: i64 = 0;
    let mut peso_g: i64 = 0;
    let mut volumen_cm3: i64 = 0;
    let mut detalles = Vec::with_capacity(lines.len());
    for (&(pid, vid), &qty) in &lines {
        let p = products.get(&pid).ok_or_else(|| ApiError::not_found(format!("Producto {pid} no encontrado")))?;
        if !p.estatus {
            return Err(ApiError::bad_request(format!("El producto '{}' no está disponible", p.nombre)));
        }
        let unit_peso = p.peso_gramos.map(i64::from).filter(|g| *g > 0).unwrap_or(PESO_DEFECTO_G);
        peso_g += unit_peso * qty as i64;
        if let (Some(l), Some(a), Some(h)) = (p.largo_cm, p.ancho_cm, p.alto_cm) {
            volumen_cm3 += i64::from(l) * i64::from(a) * i64::from(h) * qty as i64;
        }
        let (precio_unitario, stock, nombre_variante) = match vid {
            Some(vid) => {
                let v = variants.get(&vid).ok_or_else(|| ApiError::not_found(format!("Variante {vid} no encontrada")))?;
                if v.producto_id != pid {
                    return Err(ApiError::bad_request("La variante no pertenece a ese producto"));
                }
                (p.precio.checked_add(v.precio_extra).ok_or_else(overflow)?, v.stock, Some(format!("{}: {}", v.nombre, v.valor)))
            }
            None => (p.precio, p.stock, None),
        };
        if stock < qty {
            return Err(ApiError::bad_request(format!(
                "Stock insuficiente para '{}': disponibles {stock}, solicitados {qty}", p.nombre)));
        }
        let linea = precio_unitario.checked_mul(qty as i64).ok_or_else(overflow)?;
        subtotal = subtotal.checked_add(linea).ok_or_else(overflow)?;
        detalles.push(NewDetalleOrden {
            orden_id: None,
            producto_id: Some(pid),
            variante_id: vid,
            nombre_producto: p.nombre.clone(),
            nombre_variante,
            cantidad: qty,
            precio_unitario,
        });
    }
    Ok(Priced { lines, detalles, subtotal, peso_g, volumen_cm3 })
}

/// Datos del pedido que influyen en el costo de envío.
pub struct ShipInput<'a> {
    pub peso_g: i64,
    pub volumen_cm3: i64,
    /// Valor declarado (para seguro del courier): subtotal con descuento.
    pub valor: i64,
    /// Comuna de destino (código del courier), si el cliente la eligió.
    pub comuna: Option<&'a str>,
    pub courier: &'a Courier,
}

/// Métodos manuales (tarifas planas) configurados para el país.
fn manual_methods(p: &PaisEnvio, base: i64) -> Vec<MetodoEnvioCotizado> {
    p.metodos.iter().map(|m| MetodoEnvioCotizado {
        id: m.id.clone(),
        nombre: m.nombre.clone(),
        costo: m.costo_para(base),
        precio_normal: m.precio,
        gratis_desde: m.gratis_desde,
        plazo: m.plazo.clone(),
        descripcion: m.descripcion.clone(),
    }).collect()
}

/// Un único método calculado por peso/tamaño (peso volumétrico). Vacío si no está configurado.
fn peso_methods(p: &PaisEnvio, base: i64, inp: &ShipInput) -> Vec<MetodoEnvioCotizado> {
    let c = p.peso.clone().unwrap_or_default();
    if c.base == 0 && c.precio_por_kg == 0 {
        return vec![]; // sin configurar -> queda "a coordinar"
    }
    let factor = if c.factor_volumetrico > 0 { c.factor_volumetrico } else { 4000 };
    let facturable_g = inp.peso_g.max(inp.volumen_cm3 * 1000 / factor).max(1);
    let kg = (facturable_g as f64 / 1000.0).ceil().max(1.0) as i64;
    let bruto = c.base + c.precio_por_kg * kg;
    let costo = match c.gratis_desde {
        Some(limite) if base >= limite => 0,
        _ => bruto,
    };
    vec![MetodoEnvioCotizado {
        id: "peso".to_string(),
        nombre: c.nombre.unwrap_or_else(|| "Envío por peso y tamaño".to_string()),
        costo,
        precio_normal: bruto,
        gratis_desde: c.gratis_desde,
        plazo: c.plazo,
        descripcion: Some(format!("{kg} kg facturables")),
    }]
}

/// Opciones cotizadas por el courier (Blue Express). Si falla o no está
/// configurado, cae a los métodos manuales del país.
fn courier_methods(p: &PaisEnvio, base: i64, inp: &ShipInput) -> Vec<MetodoEnvioCotizado> {
    let c = p.courier.clone().unwrap_or_default();
    let comuna = inp.comuna.map(str::trim).filter(|s| !s.is_empty());
    if !inp.courier.enabled() || comuna.is_none() {
        return manual_methods(p, base);
    }
    match inp.courier.quote(&c.origen, comuna.unwrap(), inp.peso_g, inp.volumen_cm3, inp.valor) {
        Ok(opts) if !opts.is_empty() => opts.into_iter().map(|o| {
            let bruto = o.precio + c.recargo.max(0);
            let costo = match c.gratis_desde {
                Some(limite) if base >= limite => 0,
                _ => bruto,
            };
            MetodoEnvioCotizado {
                id: o.codigo,
                nombre: o.nombre,
                costo,
                precio_normal: bruto,
                gratis_desde: c.gratis_desde,
                plazo: o.dias.map(|d| format!("{d} día(s) hábiles")),
                descripcion: Some("Cotizado con Blue Express".to_string()),
            }
        }).collect(),
        Ok(_) => manual_methods(p, base),
        Err(e) => {
            log::warn!("Blue Express no cotizó ({e}); se usan los métodos manuales");
            manual_methods(p, base)
        }
    }
}

/// Métodos de envío del país con su costo para este pedido, y el elegido.
fn shipping(conn: &mut PgConnection, pais: &str, metodo: Option<&str>, base: i64, inp: &ShipInput)
    -> ApiResult<(Vec<MetodoEnvioCotizado>, Option<MetodoEnvioCotizado>, bool)>
{
    let cfg = tienda_services::envio(conn)?;
    let pais = pais.trim().to_uppercase();
    let p = cfg.paises.iter().find(|p| p.codigo == pais)
        .ok_or_else(|| ApiError::bad_request(format!("No realizamos despachos a '{pais}'")))?;
    let metodos = match p.modo {
        ModoEnvio::Manual => manual_methods(p, base),
        ModoEnvio::Peso => peso_methods(p, base, inp),
        ModoEnvio::Courier => courier_methods(p, base, inp),
    };
    if metodos.is_empty() {
        return Ok((metodos, None, true));
    }
    let elegido = match metodo {
        Some(id) => Some(metodos.iter().find(|m| m.id == id)
            .ok_or_else(|| ApiError::bad_request("El método de envío ya no está disponible; vuelve a elegirlo"))?.clone()),
        None => None,
    };
    Ok((metodos, elegido, false))
}

pub fn quote(conn: &mut PgConnection, user_id: Uuid, req: QuoteRequest, courier: &Courier) -> ApiResult<Cotizacion> {
    let (_, lines) = collect_lines(conn, user_id, req.items)?;
    let priced = price_lines(conn, lines, false)?;
    let (descuento, cupon, cupon_error) = match req.cupon.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        Some(codigo) => match find_coupon(conn, codigo, false) {
            Ok(c) => match c.descuento_para(priced.subtotal, now()) {
                Ok(d) => (d, Some(c.codigo), None),
                Err(msg) => (0, None, Some(msg)),
            },
            Err(_) => (0, None, Some("El cupón no existe".to_string())),
        },
        None => (0, None, None),
    };
    let base = priced.subtotal - descuento;
    let (_, descuento_puntos, mut puntos) = match puntos_para(conn, user_id, req.puntos, base) {
        Ok(r) => r,
        Err(e) => {
            let (_, _, mut info) = puntos_para(conn, user_id, 0, base)?;
            info.error = Some(e.to_string().split_once(": ").map(|(_, m)| m.to_string()).unwrap_or_default());
            (0, 0, info)
        }
    };
    let cfg_p = puntos_services::config(conn)?;
    puntos.ganara = cfg_p.ganados_por(base - descuento_puntos);
    let inp = ShipInput { peso_g: priced.peso_g, volumen_cm3: priced.volumen_cm3, valor: base, comuna: req.comuna.as_deref(), courier };
    let (metodos_envio, elegido, a_coordinar) = match req.pais.as_deref().filter(|p| !p.trim().is_empty()) {
        Some(pais) => shipping(conn, pais, req.metodo_envio.as_deref(), base, &inp)?,
        None => (vec![], None, false),
    };
    let costo_envio = elegido.as_ref().map_or(0, |m| m.costo);
    Ok(Cotizacion {
        subtotal: priced.subtotal,
        descuento,
        cupon,
        cupon_error,
        costo_envio,
        metodo_envio: elegido.map(|m| m.nombre),
        envio_a_coordinar: a_coordinar,
        metodos_envio,
        puntos,
        total: base - descuento_puntos + costo_envio,
    })
}

pub fn checkout(conn: &mut PgConnection, user_id: Uuid, req: CheckoutRequest, mp_enabled: bool, courier: &Courier) -> ApiResult<OrdenDetalle> {
    // Validaciones que no dependen de la BD
    let nombre_comprador = required_text("nombre_comprador", &req.nombre_comprador, 255)?;
    let email_comprador = required_text("email_comprador", &req.email_comprador, 255)?.to_lowercase();
    if !email_comprador.contains('@') || !email_comprador.contains('.') {
        return Err(ApiError::bad_request("El email del comprador no es válido"));
    }
    let telefono_comprador = optional_text("telefono_comprador", req.telefono_comprador, 50)?;

    conn.transaction(|conn| {
        // Documento tributario: sólo para Chile. Factura exige datos de facturación.
        let pais_doc = req.pais.trim().to_uppercase();
        let doc = if pais_doc == "CL" {
            match req.tipo_documento.as_deref().unwrap_or("boleta").trim() {
                "boleta" => DocumentoFactura { tipo: Some("boleta".into()), ..Default::default() },
                "factura" => DocumentoFactura {
                    tipo: Some("factura".into()),
                    rut: Some(validate_rut(req.factura_rut.as_deref().unwrap_or(""))?),
                    razon_social: Some(required_text("razón social", req.factura_razon_social.as_deref().unwrap_or(""), 255)?),
                    giro: Some(required_text("giro", req.factura_giro.as_deref().unwrap_or(""), 255)?),
                    direccion: Some(required_text("dirección de facturación", req.factura_direccion.as_deref().unwrap_or(""), 2000)?),
                },
                _ => return Err(ApiError::bad_request("tipo_documento debe ser 'boleta' o 'factura'")),
            }
        } else {
            DocumentoFactura::default()
        };
        let (from_cart, lines) = collect_lines(conn, user_id, req.items)?;

        // Dirección de envío
        let direccion_envio = match (req.direccion_id, req.direccion_envio) {
            (Some(did), _) => direcciones::table
                .filter(direcciones::id.eq(did).and(direcciones::user_id.eq(user_id)))
                .select(Direccion::as_select())
                .first(conn)
                .or_not_found("Dirección")?
                .formatted(),
            (None, Some(texto)) => required_text("direccion_envio", &texto, 2000)?,
            (None, None) => return Err(ApiError::bad_request("Indica 'direccion_id' o 'direccion_envio'")),
        };

        // Moneda
        let monedas = tienda_services::monedas(conn)?;
        let codigo_moneda = req.moneda.as_deref().map(|m| m.trim().to_uppercase()).unwrap_or_else(|| monedas.base.clone());
        let (_, tasa_cambio) = monedas.find(&codigo_moneda)
            .ok_or_else(|| ApiError::bad_request(format!("La moneda '{codigo_moneda}' no está disponible")))?;

        // Método de pago (sólo los disponibles para el país de entrega)
        let pais_pago = req.pais.trim().to_uppercase();
        let pagos_cfg = tienda_services::pagos(conn)?;
        let disponibles: Vec<_> = pagos_cfg.metodos.iter()
            .filter(|m| m.disponible_en(&pais_pago) && (m.tipo != "mercadopago" || mp_enabled))
            .collect();
        let (metodo_pago, pago_proveedor) = if disponibles.is_empty() {
            (PAGO_A_COORDINAR.to_string(), None)
        } else {
            let id = req.metodo_pago.as_deref().ok_or_else(|| ApiError::bad_request("Elige un método de pago"))?;
            let m = disponibles.iter().find(|m| m.id == id)
                .ok_or_else(|| ApiError::bad_request("El método de pago no está disponible para ese país"))?;
            (m.nombre.clone(), (m.tipo == "mercadopago").then(|| "mercadopago".to_string()))
        };
        // Nota: Mercado Pago siempre cobra el total en la moneda base; la moneda elegida sólo cambia cómo se muestran los montos.

        // Precios (con bloqueo de filas)
        let Priced { lines, mut detalles, subtotal, peso_g, volumen_cm3 } = price_lines(conn, lines, true)?;

        // Cupón
        let (cupon_id, descuento) = match req.cupon.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            Some(codigo) => {
                let cupon = find_coupon(conn, codigo, true)?;
                let descuento = cupon.descuento_para(subtotal, now()).map_err(ApiError::bad_request)?;
                diesel::update(cupones::table.find(cupon.id))
                    .set(cupones::uso_actual.eq(cupones::uso_actual + 1))
                    .execute(conn)?;
                (Some(cupon.id), descuento)
            }
            None => (None, 0),
        };

        // Puntos (con el usuario bloqueado: el saldo no se puede usar dos veces en paralelo)
        if req.puntos > 0 {
            puntos_services::lock_user(conn, user_id)?;
        }
        let (puntos_usados, descuento_puntos, _) = puntos_para(conn, user_id, req.puntos, subtotal - descuento)?;

        // Envío (el umbral de envío gratis se calcula antes de descontar puntos)
        let pais = req.pais.trim().to_uppercase();
        let comuna = req.comuna.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let inp = ShipInput { peso_g, volumen_cm3, valor: subtotal - descuento, comuna, courier };
        let (_, elegido, a_coordinar) = shipping(conn, &pais, req.metodo_envio.as_deref(), subtotal - descuento, &inp)?;
        let (metodo_envio, costo_envio) = match (elegido, a_coordinar) {
            (Some(m), _) => (m.nombre, m.costo),
            (None, true) => (ENVIO_A_COORDINAR.to_string(), 0),
            (None, false) => return Err(ApiError::bad_request("Elige un método de envío")),
        };
        let total = (subtotal - descuento - descuento_puntos).checked_add(costo_envio).ok_or_else(overflow)?;

        // Crear orden y detalles
        let orden: Orden = diesel::insert_into(ordenes::table)
            .values(NewOrden {
                user_id: Some(user_id),
                subtotal,
                descuento,
                total,
                cupon_id,
                estatus: EstatusOrden::Pendiente.as_str().to_string(),
                metodo_pago: Some(metodo_pago),
                direccion_envio: Some(direccion_envio),
                costo_envio,
                metodo_envio: Some(metodo_envio),
                pais: Some(pais),
                moneda: codigo_moneda,
                tasa_cambio,
                nombre_comprador: Some(nombre_comprador),
                email_comprador: Some(email_comprador),
                telefono_comprador,
                pago_proveedor,
                puntos_usados: puntos_usados as i32,
                descuento_puntos,
                tipo_documento: doc.tipo,
                factura_rut: doc.rut,
                factura_razon_social: doc.razon_social,
                factura_giro: doc.giro,
                factura_direccion: doc.direccion,
                comuna_destino: comuna.map(|c| c.to_string()),
            })
            .returning(Orden::as_returning())
            .get_result(conn)?;
        for d in detalles.iter_mut() {
            d.orden_id = Some(orden.id);
        }
        diesel::insert_into(detalle_orden::table).values(&detalles).execute(conn)?;
        if puntos_usados > 0 {
            puntos_services::redeem(conn, user_id, orden.id, puntos_usados)?;
        }

        // Descontar stock (las filas ya están bloqueadas)
        for (&(pid, vid), &qty) in &lines {
            match vid {
                Some(vid) => diesel::update(product_variants::table.find(vid))
                    .set((product_variants::stock.eq(product_variants::stock - qty), product_variants::updated_at.eq(now())))
                    .execute(conn)?,
                None => diesel::update(productos::table.find(pid))
                    .set((productos::stock.eq(productos::stock - qty), productos::updated_at.eq(now())))
                    .execute(conn)?,
            };
        }

        if from_cart {
            diesel::delete(carrito::table.filter(carrito::user_id.eq(user_id))).execute(conn)?;
        }

        detail_of(conn, orden)
    })
}

// ---------- Consultas ----------

pub fn get_for(conn: &mut PgConnection, id: Uuid, user: &AuthUser) -> ApiResult<OrdenDetalle> {
    let orden: Orden = ordenes::table.find(id).select(Orden::as_select()).first(conn).or_not_found("Orden")?;
    user.require_owner_or_admin(orden.user_id)
        .map_err(|_| ApiError::not_found("Orden no encontrada"))?; // no revelar que existe
    detail_of(conn, orden)
}

pub fn list(conn: &mut PgConnection, filter: OrderFilter, p: Pagination) -> ApiResult<Paginated<Orden>> {
    let build = || {
        let mut q = ordenes::table.into_boxed();
        if let Some(e) = filter.estatus {
            q = q.filter(ordenes::estatus.eq(e.as_str()));
        }
        if let Some(u) = filter.user_id {
            q = q.filter(ordenes::user_id.eq(u));
        }
        q
    };
    let total: i64 = build().count().get_result(conn)?;
    let items = build()
        .order(ordenes::created_at.desc())
        .limit(p.per_page())
        .offset(p.offset())
        .select(Orden::as_select())
        .load(conn)?;
    Ok(Paginated::new(items, &p, total))
}

// ---------- Cambios de estado ----------

fn restore_stock_and_coupon(conn: &mut PgConnection, orden: &Orden) -> ApiResult<()> {
    let detalles: Vec<DetalleOrden> = detalle_orden::table
        .filter(detalle_orden::orden_id.eq(orden.id))
        .select(DetalleOrden::as_select())
        .load(conn)?;
    for d in detalles {
        match (d.variante_id, d.producto_id) {
            (Some(vid), _) => {
                diesel::update(product_variants::table.find(vid))
                    .set((product_variants::stock.eq(product_variants::stock + d.cantidad), product_variants::updated_at.eq(now())))
                    .execute(conn)?;
            }
            (None, Some(pid)) => {
                diesel::update(productos::table.find(pid))
                    .set((productos::stock.eq(productos::stock + d.cantidad), productos::updated_at.eq(now())))
                    .execute(conn)?;
            }
            // El producto fue eliminado: no hay stock que devolver
            (None, None) => {}
        }
    }
    if let Some(cid) = orden.cupon_id {
        diesel::update(cupones::table.find(cid).filter(cupones::uso_actual.gt(0)))
            .set(cupones::uso_actual.eq(cupones::uso_actual - 1))
            .execute(conn)?;
    }
    Ok(())
}

fn transition(conn: &mut PgConnection, orden: Orden, next: EstatusOrden) -> ApiResult<OrdenDetalle> {
    let current = EstatusOrden::parse(&orden.estatus)?;
    if !current.can_transition_to(next) {
        return Err(ApiError::conflict(format!(
            "No se puede pasar una orden de '{}' a '{}'", current.as_str(), next.as_str())));
    }
    if next == EstatusOrden::Cancelado {
        restore_stock_and_coupon(conn, &orden)?;
        puntos_services::on_cancel(conn, &orden)?;
    }
    diesel::update(ordenes::table.find(orden.id))
        .set((ordenes::estatus.eq(next.as_str()), ordenes::updated_at.eq(now())))
        .execute(conn)?;
    if next == EstatusOrden::Pagado {
        puntos_services::on_paid(conn, orden.id)?;
    }
    let orden = ordenes::table.find(orden.id).select(Orden::as_select()).first(conn)?;
    detail_of(conn, orden)
}

/// El dueño puede cancelar mientras esté pendiente; un admin también si ya está pagada.
pub fn cancel(conn: &mut PgConnection, id: Uuid, user: AuthUser) -> ApiResult<OrdenDetalle> {
    conn.transaction(|conn| {
        let orden = get_order_for_update(conn, id)?;
        user.require_owner_or_admin(orden.user_id).map_err(|_| ApiError::not_found("Orden no encontrada"))?;
        if !user.is_admin() && orden.estatus != EstatusOrden::Pendiente.as_str() {
            return Err(ApiError::conflict("Sólo puedes cancelar órdenes pendientes. Contacta a soporte"));
        }
        transition(conn, orden, EstatusOrden::Cancelado)
    })
}

pub fn set_status(conn: &mut PgConnection, id: Uuid, next: EstatusOrden) -> ApiResult<OrdenDetalle> {
    conn.transaction(|conn| {
        let orden = get_order_for_update(conn, id)?;
        transition(conn, orden, next)
    })
}

/// Registra un pago (lo hace un admin o, en el futuro, el webhook de la pasarela).
/// Cuando los pagos completados cubren el total, la orden pasa a 'pagado'.
/// Devuelve el detalle y `true` si la orden pasó a pagada en esta llamada.
pub fn add_payment(conn: &mut PgConnection, id: Uuid, req: PagoRequest) -> ApiResult<(OrdenDetalle, bool)> {
    if req.monto <= 0 {
        return Err(ApiError::bad_request("El monto debe ser mayor a 0"));
    }
    conn.transaction(|conn| {
        let orden = get_order_for_update(conn, id)?;
        if orden.estatus == EstatusOrden::Cancelado.as_str() {
            return Err(ApiError::conflict("La orden está cancelada"));
        }
        let metodo = optional_text("metodo", req.metodo, 50)?;
        diesel::insert_into(pagos::table)
            .values(NewPago {
                orden_id: Some(id),
                monto: req.monto,
                metodo: metodo.clone(),
                estatus: Some(req.estatus.as_str().to_string()),
                referencia_externa: optional_text("referencia_externa", req.referencia_externa, 255)?,
                proveedor: None,
            })
            .execute(conn)?;

        let pagado: i64 = pagos::table
            .filter(pagos::orden_id.eq(id).and(pagos::estatus.eq(EstatusPago::Completado.as_str())))
            .select(pagos::monto)
            .load::<i64>(conn)?
            .into_iter()
            .sum();
        let just_paid = orden.estatus == EstatusOrden::Pendiente.as_str() && pagado >= orden.total;
        if just_paid {
            diesel::update(ordenes::table.find(id))
                .set((
                    ordenes::estatus.eq(EstatusOrden::Pagado.as_str()),
                    ordenes::metodo_pago.eq(metodo.or(orden.metodo_pago.clone())),
                    ordenes::updated_at.eq(now()),
                ))
                .execute(conn)?;
            puntos_services::on_paid(conn, id)?;
        }
        let orden = ordenes::table.find(id).select(Orden::as_select()).first(conn)?;
        Ok((detail_of(conn, orden)?, just_paid))
    })
}

/// Crea o actualiza la información de envío de la orden.
pub fn set_shipping(conn: &mut PgConnection, id: Uuid, mut data: EnvioData) -> ApiResult<OrdenDetalle> {
    conn.transaction(|conn| {
        let orden = get_order_for_update(conn, id)?;
        if orden.estatus == EstatusOrden::Cancelado.as_str() {
            return Err(ApiError::conflict("La orden está cancelada"));
        }
        data.orden_id = Some(id);
        diesel::insert_into(envios::table)
            .values(&data)
            .on_conflict(envios::orden_id)
            .do_update()
            .set(&data)
            .execute(conn)?;
        detail_of(conn, orden)
    })
}


// ---------- Pagos online (proveedor externo) ----------

/// Registra (o actualiza) un pago informado por un proveedor y, si queda aprobado
/// y cubre el total en la moneda base, marca la orden como pagada.
#[allow(clippy::too_many_arguments)]
pub fn apply_provider_payment(
    conn: &mut PgConnection,
    proveedor: &str,
    referencia: &str,
    orden_id: Uuid,
    monto: i64,
    moneda: &str,
    estado_proveedor: &str,
    metodo: Option<String>,
) -> ApiResult<(OrdenDetalle, bool)> {
    let estatus = match estado_proveedor {
        "approved" => "completado",
        "rejected" | "cancelled" => "fallido",
        "refunded" | "charged_back" => "reembolsado",
        _ => "pendiente",
    };
    conn.transaction(|conn| {
        let mut just_paid = false;
        let orden = get_order_for_update(conn, orden_id)?;
        let base = tienda_services::monedas(conn)?.base;
        let existing: Option<Uuid> = pagos::table
            .filter(pagos::proveedor.eq(proveedor).and(pagos::referencia_externa.eq(referencia)))
            .select(pagos::id)
            .first(conn)
            .optional()?;
        match existing {
            Some(pid) => {
                diesel::update(pagos::table.find(pid)).set((pagos::estatus.eq(estatus), pagos::monto.eq(monto))).execute(conn)?;
            }
            None => {
                diesel::insert_into(pagos::table).values(NewPago {
                    orden_id: Some(orden_id),
                    monto,
                    metodo: metodo.clone().or(Some(proveedor.to_string())),
                    estatus: Some(estatus.to_string()),
                    referencia_externa: Some(referencia.to_string()),
                    proveedor: Some(proveedor.to_string()),
                }).execute(conn)?;
            }
        }
        if estatus == "completado" {
            if !moneda.eq_ignore_ascii_case(&base) {
                log::warn!("Pago {referencia} en {moneda} distinto a la moneda base {base}: requiere revisión manual");
            } else if orden.estatus == EstatusOrden::Cancelado.as_str() {
                log::warn!("Pago aprobado {referencia} para la orden cancelada {orden_id}: requiere devolución manual");
            } else if orden.estatus == EstatusOrden::Pendiente.as_str() {
                let pagado: i64 = pagos::table
                    .filter(pagos::orden_id.eq(orden_id).and(pagos::estatus.eq("completado")))
                    .select(pagos::monto)
                    .load::<i64>(conn)?
                    .into_iter()
                    .sum();
                if pagado >= orden.total {
                    diesel::update(ordenes::table.find(orden_id))
                        .set((ordenes::estatus.eq(EstatusOrden::Pagado.as_str()), ordenes::updated_at.eq(now())))
                        .execute(conn)?;
                    puntos_services::on_paid(conn, orden_id)?;
                    just_paid = true;
                    log::info!("Orden {orden_id} pagada vía {proveedor} ({referencia})");
                }
            }
        }
        let orden = ordenes::table.find(orden_id).select(Orden::as_select()).first(conn)?;
        Ok((detail_of(conn, orden)?, just_paid))
    })
}

/// Orden del usuario lista para pagar online (pendiente y con proveedor).
pub fn payable_order(conn: &mut PgConnection, id: Uuid, user: &AuthUser) -> ApiResult<Orden> {
    let orden: Orden = ordenes::table.find(id).select(Orden::as_select()).first(conn).or_not_found("Orden")?;
    user.require_owner_or_admin(orden.user_id).map_err(|_| ApiError::not_found("Orden no encontrada"))?;
    if orden.estatus != EstatusOrden::Pendiente.as_str() {
        return Err(ApiError::conflict("Este pedido ya no está pendiente de pago"));
    }
    if orden.pago_proveedor.as_deref() != Some("mercadopago") {
        return Err(ApiError::bad_request("Este pedido no usa pago online"));
    }
    Ok(orden)
}


/// Datos de facturación resueltos para una orden.
#[derive(Default)]
struct DocumentoFactura {
    tipo: Option<String>,
    rut: Option<String>,
    razon_social: Option<String>,
    giro: Option<String>,
    direccion: Option<String>,
}

/// Asocia el documento emitido (boleta/factura, PDF o imagen) a un pedido.
pub fn set_document(conn: &mut PgConnection, orden_id: Uuid, url: String) -> ApiResult<OrdenDetalle> {
    let orden = get_order_for_update(conn, orden_id)?;
    diesel::update(ordenes::table.find(orden.id))
        .set((ordenes::documento_url.eq(url), ordenes::documento_subido_at.eq(now()), ordenes::updated_at.eq(now())))
        .execute(conn)?;
    let orden = ordenes::table.find(orden_id).select(Orden::as_select()).first(conn)?;
    detail_of(conn, orden)
}
