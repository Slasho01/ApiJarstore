//! Programa de puntos (libro de movimientos; el saldo es la suma).
use crate::errors::{ApiError, ApiResult};
use crate::models::orden::Orden;
use crate::models::tienda::{MovimientoPuntos, PuntosConfig};
use crate::schema::{configuracion, ordenes, puntos_movimientos, users};
use crate::services::common::{Paginated, Pagination};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

pub fn config(conn: &mut PgConnection) -> ApiResult<PuntosConfig> {
    let valor: Option<serde_json::Value> = configuracion::table.find("puntos").select(configuracion::valor).first(conn).optional()?;
    match valor {
        Some(v) => serde_json::from_value(v).map_err(|e| ApiError::internal(format!("Configuración de puntos corrupta: {e}"))),
        None => Ok(PuntosConfig { activo: false, nombre: "Puntos".into(), monto_base: 1000, puntos_por_monto: 1, valor_punto: 10, minimo_canje: 0, maximo_porcentaje: 0 }),
    }
}

pub fn saldo(conn: &mut PgConnection, user_id: Uuid) -> ApiResult<i64> {
    let s: Option<i64> = puntos_movimientos::table
        .filter(puntos_movimientos::user_id.eq(user_id))
        .select(diesel::dsl::sum(puntos_movimientos::puntos))
        .first(conn)?;
    Ok(s.unwrap_or(0))
}

/// Bloquea al usuario durante la transacción (evita canjear dos veces el mismo saldo en paralelo).
pub fn lock_user(conn: &mut PgConnection, user_id: Uuid) -> ApiResult<()> {
    users::table.find(user_id).select(users::id).for_update().first::<Uuid>(conn)?;
    Ok(())
}

/// Base sobre la que se ganan puntos: productos menos cupón menos puntos usados (sin envío).
pub fn gasto_que_suma(o: &Orden) -> i64 {
    (o.subtotal - o.descuento - o.descuento_puntos).max(0)
}

fn insert(conn: &mut PgConnection, user_id: Uuid, orden_id: Option<Uuid>, tipo: &str, puntos: i64, desc: String) -> ApiResult<bool> {
    if puntos == 0 { return Ok(false); }
    let n = diesel::insert_into(puntos_movimientos::table)
        .values((
            puntos_movimientos::user_id.eq(user_id),
            puntos_movimientos::orden_id.eq(orden_id),
            puntos_movimientos::tipo.eq(tipo),
            puntos_movimientos::puntos.eq(i32::try_from(puntos).map_err(|_| ApiError::bad_request("Cantidad de puntos fuera de rango"))?),
            puntos_movimientos::descripcion.eq(desc),
        ))
        .on_conflict_do_nothing()
        .execute(conn)?;
    Ok(n > 0)
}

fn short(id: Uuid) -> String { id.to_string()[..8].to_uppercase() }

/// Canje al crear la orden.
pub fn redeem(conn: &mut PgConnection, user_id: Uuid, orden_id: Uuid, puntos: i64) -> ApiResult<()> {
    insert(conn, user_id, Some(orden_id), "canjeados", -puntos, format!("Canje en el pedido #{}", short(orden_id)))?;
    Ok(())
}

/// Acredita los puntos ganados cuando la orden queda pagada (idempotente).
pub fn on_paid(conn: &mut PgConnection, orden_id: Uuid) -> ApiResult<()> {
    let o: Orden = ordenes::table.find(orden_id).select(Orden::as_select()).first(conn)?;
    let Some(user_id) = o.user_id else { return Ok(()) };
    let cfg = config(conn)?;
    let pts = cfg.ganados_por(gasto_que_suma(&o));
    if pts > 0 && insert(conn, user_id, Some(orden_id), "ganados", pts, format!("Compra del pedido #{}", short(orden_id)))? {
        diesel::update(ordenes::table.find(orden_id)).set(ordenes::puntos_ganados.eq(pts as i32)).execute(conn)?;
    }
    Ok(())
}

/// Al cancelar: devuelve los puntos canjeados y revierte los ganados (idempotente).
pub fn on_cancel(conn: &mut PgConnection, o: &Orden) -> ApiResult<()> {
    let Some(user_id) = o.user_id else { return Ok(()) };
    let s = short(o.id);
    if o.puntos_usados > 0 {
        insert(conn, user_id, Some(o.id), "devueltos", o.puntos_usados as i64, format!("Devolución por cancelación del pedido #{s}"))?;
    }
    if o.puntos_ganados > 0 {
        insert(conn, user_id, Some(o.id), "revertidos", -(o.puntos_ganados as i64), format!("Reversión por cancelación del pedido #{s}"))?;
    }
    Ok(())
}

pub fn adjust(conn: &mut PgConnection, user_id: Uuid, puntos: i64, motivo: &str) -> ApiResult<i64> {
    if puntos == 0 || puntos.abs() > 1_000_000 {
        return Err(ApiError::bad_request("El ajuste debe ser distinto de 0 y menor a 1.000.000"));
    }
    let motivo = crate::services::common::required_text("motivo", motivo, 200)?;
    conn.transaction(|conn| {
        lock_user(conn, user_id)?;
        if saldo(conn, user_id)? + puntos < 0 {
            return Err(ApiError::bad_request("El ajuste dejaría el saldo negativo"));
        }
        insert(conn, user_id, None, "ajuste", puntos, motivo)?;
        saldo(conn, user_id)
    })
}

#[derive(Serialize)]
pub struct Resumen {
    pub config: PuntosConfig,
    pub saldo: i64,
    /// Equivalente en dinero del saldo (moneda base)
    pub valor_saldo: i64,
    /// Puntos que se acreditarán cuando se paguen los pedidos pendientes
    pub por_acreditar: i64,
    pub movimientos: Paginated<MovimientoPuntos>,
}

pub fn resumen(conn: &mut PgConnection, user_id: Uuid, p: Pagination) -> ApiResult<Resumen> {
    let cfg = config(conn)?;
    let saldo = saldo(conn, user_id)?;
    let pendientes: Vec<Orden> = ordenes::table
        .filter(ordenes::user_id.eq(user_id).and(ordenes::estatus.eq("pendiente")))
        .select(Orden::as_select())
        .load(conn)?;
    let por_acreditar = pendientes.iter().map(|o| cfg.ganados_por(gasto_que_suma(o))).sum();
    let total: i64 = puntos_movimientos::table.filter(puntos_movimientos::user_id.eq(user_id)).count().get_result(conn)?;
    let items = puntos_movimientos::table
        .filter(puntos_movimientos::user_id.eq(user_id))
        .order(puntos_movimientos::created_at.desc())
        .limit(p.per_page())
        .offset(p.offset())
        .select(MovimientoPuntos::as_select())
        .load(conn)?;
    Ok(Resumen { valor_saldo: saldo.max(0) * cfg.valor_punto, saldo, por_acreditar, movimientos: Paginated::new(items, &p, total), config: cfg })
}
