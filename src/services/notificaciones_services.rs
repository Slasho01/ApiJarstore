//! Notificaciones de conversión: "avísame cuando vuelva" (reposición de stock),
//! carrito abandonado y stock bajo para el panel.
use crate::errors::{ApiError, ApiResult};
use crate::schema::{avisos_stock, carrito, carrito_avisos, perfil, product_variants, productos, users};
use crate::services::common::now;
use chrono::{Duration, NaiveDateTime};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

// ----------------------------- Avísame cuando vuelva -----------------------------

/// Suscribe un correo a la reposición de un producto (o variante) agotado.
pub fn subscribe_stock(conn: &mut PgConnection, producto_id: Uuid, variante_id: Option<Uuid>, email: &str, user_id: Option<Uuid>) -> ApiResult<()> {
    let email = email.trim().to_lowercase();
    if !email.contains('@') || email.len() > 255 {
        return Err(ApiError::bad_request("Ingresa un correo válido"));
    }
    let (p_stock, p_estatus): (i32, bool) = productos::table
        .find(producto_id)
        .select((productos::stock, productos::estatus))
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::not_found("Producto no encontrado"))?;
    if !p_estatus {
        return Err(ApiError::not_found("Producto no encontrado"));
    }
    let stock = match variante_id {
        Some(vid) => {
            let (v_stock, v_pid): (i32, Uuid) = product_variants::table
                .find(vid)
                .select((product_variants::stock, product_variants::producto_id))
                .first(conn)
                .optional()?
                .ok_or_else(|| ApiError::not_found("Variante no encontrada"))?;
            if v_pid != producto_id {
                return Err(ApiError::bad_request("La variante no pertenece a ese producto"));
            }
            v_stock
        }
        None => p_stock,
    };
    if stock > 0 {
        return Err(ApiError::bad_request("El producto está disponible: puedes comprarlo ahora"));
    }
    // Evitar duplicados: si ya hay un aviso pendiente para el mismo correo, no insertar otro.
    let base = avisos_stock::table
        .filter(avisos_stock::producto_id.eq(producto_id))
        .filter(avisos_stock::email.eq(&email))
        .filter(avisos_stock::notificado_at.is_null());
    let ya: Option<Uuid> = match variante_id {
        Some(vid) => base.filter(avisos_stock::variante_id.eq(vid)).select(avisos_stock::id).first(conn).optional()?,
        None => base.filter(avisos_stock::variante_id.is_null()).select(avisos_stock::id).first(conn).optional()?,
    };
    if ya.is_some() {
        return Ok(());
    }
    diesel::insert_into(avisos_stock::table)
        .values((
            avisos_stock::producto_id.eq(producto_id),
            avisos_stock::variante_id.eq(variante_id),
            avisos_stock::email.eq(&email),
            avisos_stock::user_id.eq(user_id),
        ))
        .execute(conn)?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct StockAlert {
    pub email: String,
    pub producto_nombre: String,
    pub slug: String,
    pub variante: Option<String>,
}

/// (aviso_id, email, p.nombre, p.slug, p.stock, p.estatus, v.nombre, v.valor, v.stock)
type StockRow = (Uuid, String, String, String, i32, bool, Option<String>, Option<String>, Option<i32>);

/// Avisos pendientes cuyo producto/variante ya volvió a tener stock. Los marca
/// como notificados y los devuelve para enviar los correos.
pub fn due_stock_alerts(conn: &mut PgConnection) -> ApiResult<Vec<StockAlert>> {
    let rows: Vec<StockRow> = avisos_stock::table
        .inner_join(productos::table)
        .left_join(product_variants::table.on(avisos_stock::variante_id.eq(product_variants::id.nullable())))
        .filter(avisos_stock::notificado_at.is_null())
        .select((
            avisos_stock::id,
            avisos_stock::email,
            productos::nombre,
            productos::slug,
            productos::stock,
            productos::estatus,
            product_variants::nombre.nullable(),
            product_variants::valor.nullable(),
            product_variants::stock.nullable(),
        ))
        .load(conn)?;

    let mut alerts = Vec::new();
    let mut ids = Vec::new();
    for (id, email, nombre, slug, p_stock, estatus, v_nombre, v_valor, v_stock) in rows {
        if !estatus {
            continue;
        }
        let disponible = match v_stock {
            Some(s) => s > 0,       // aviso de una variante
            None => p_stock > 0,    // aviso del producto
        };
        if !disponible {
            continue;
        }
        ids.push(id);
        alerts.push(StockAlert {
            email,
            producto_nombre: nombre,
            slug,
            variante: match (v_nombre, v_valor) {
                (Some(n), Some(v)) => Some(format!("{n}: {v}")),
                _ => None,
            },
        });
    }
    if !ids.is_empty() {
        diesel::update(avisos_stock::table.filter(avisos_stock::id.eq_any(&ids)))
            .set(avisos_stock::notificado_at.eq(now()))
            .execute(conn)?;
    }
    Ok(alerts)
}

// ----------------------------- Carrito abandonado -----------------------------

#[derive(Debug, Clone)]
pub struct CartReminder {
    pub email: String,
    pub nombre: Option<String>,
    pub items: Vec<(String, i32)>,
}

/// Carritos sin actividad entre `min_horas` y `max_dias` cuyo dueño no recibió
/// aún un recordatorio por esa actividad. Registra el envío y los devuelve.
pub fn due_cart_reminders(conn: &mut PgConnection, min_horas: i64, max_dias: i64) -> ApiResult<Vec<CartReminder>> {
    let ahora = now();
    let mas_reciente_que = ahora - Duration::hours(min_horas); // la última actividad debe ser anterior a esto
    let no_tan_viejo_que = ahora - Duration::days(max_dias);

    let candidatos: Vec<(Uuid, Option<NaiveDateTime>)> = carrito::table
        .group_by(carrito::user_id)
        .select((carrito::user_id, diesel::dsl::max(carrito::created_at)))
        .load(conn)?;

    let mut reminders = Vec::new();
    for (uid, last) in candidatos {
        let Some(last) = last else { continue };
        if last >= mas_reciente_que || last <= no_tan_viejo_que {
            continue;
        }
        let prev: Option<NaiveDateTime> = carrito_avisos::table
            .find(uid)
            .select(carrito_avisos::enviado_at)
            .first(conn)
            .optional()?;
        if prev.is_some_and(|p| p >= last) {
            continue; // ya se avisó por esta actividad
        }
        let email: Option<String> = users::table.find(uid).select(users::email).first(conn).optional()?;
        let Some(email) = email else { continue };
        let nombre: Option<String> = perfil::table
            .filter(perfil::user_id.eq(uid))
            .select(perfil::nombre)
            .first(conn)
            .optional()?;
        let items: Vec<(String, i32)> = carrito::table
            .inner_join(productos::table)
            .filter(carrito::user_id.eq(uid))
            .select((productos::nombre, carrito::cantidad))
            .load(conn)?;
        if items.is_empty() {
            continue;
        }
        diesel::insert_into(carrito_avisos::table)
            .values((carrito_avisos::user_id.eq(uid), carrito_avisos::enviado_at.eq(ahora)))
            .on_conflict(carrito_avisos::user_id)
            .do_update()
            .set(carrito_avisos::enviado_at.eq(ahora))
            .execute(conn)?;
        reminders.push(CartReminder { email, nombre, items });
    }
    Ok(reminders)
}

// ----------------------------- Stock bajo (panel) -----------------------------

#[derive(Debug, Serialize)]
pub struct LowStockItem {
    pub producto_id: Uuid,
    pub nombre: String,
    pub slug: String,
    pub variante: Option<String>,
    pub stock: i32,
}

/// Productos y variantes con stock por debajo (o igual) del umbral, visibles.
pub fn low_stock(conn: &mut PgConnection, umbral: i32) -> ApiResult<Vec<LowStockItem>> {
    let umbral = umbral.clamp(0, 100000);
    // Productos sin variantes con stock bajo
    let prods: Vec<(Uuid, String, String, i32)> = productos::table
        .filter(productos::estatus.eq(true))
        .filter(productos::stock.le(umbral))
        .select((productos::id, productos::nombre, productos::slug, productos::stock))
        .order(productos::stock.asc())
        .load(conn)?;
    let mut items: Vec<LowStockItem> = prods.into_iter().map(|(id, nombre, slug, stock)| LowStockItem {
        producto_id: id, nombre, slug, variante: None, stock,
    }).collect();
    // Variantes con stock bajo
    let vars: Vec<(Uuid, String, String, String, String, i32)> = product_variants::table
        .inner_join(productos::table)
        .filter(productos::estatus.eq(true))
        .filter(product_variants::stock.le(umbral))
        .select((productos::id, productos::nombre, productos::slug, product_variants::nombre, product_variants::valor, product_variants::stock))
        .order(product_variants::stock.asc())
        .load(conn)?;
    for (pid, pnombre, slug, vn, vv, stock) in vars {
        items.push(LowStockItem { producto_id: pid, nombre: pnombre, slug, variante: Some(format!("{vn}: {vv}")), stock });
    }
    items.sort_by_key(|i| i.stock);
    Ok(items)
}
