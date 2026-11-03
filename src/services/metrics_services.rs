//! Métricas de ventas para el panel de administración.
use crate::errors::{ApiError, ApiResult};
use chrono::{Duration, NaiveDate, NaiveDateTime};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Date, Double, Nullable, Text, Timestamp};
use serde::{Deserialize, Serialize};

/// Estados que cuentan como venta concretada.
const VENDIDOS: &str = "('pagado','enviado','entregado')";

#[derive(Deserialize)]
pub struct RangoQuery {
    pub desde: Option<String>,
    pub hasta: Option<String>,
}

#[derive(Serialize)]
pub struct Metricas {
    pub desde: NaiveDate,
    pub hasta: NaiveDate,
    pub resumen: Resumen,
    pub por_dia: Vec<PuntoDia>,
    pub por_estado: Vec<EstadoConteo>,
    pub top_productos: Vec<TopProducto>,
}

#[derive(Serialize)]
pub struct Resumen {
    pub ingresos: i64,
    pub pedidos: i64,
    pub ticket_promedio: i64,
    pub unidades: i64,
    pub pendientes: i64,
    pub ingresos_pendientes: i64,
    pub clientes_nuevos: i64,
}

#[derive(Serialize, QueryableByName)]
pub struct PuntoDia {
    #[diesel(sql_type = Date)]
    pub fecha: NaiveDate,
    #[diesel(sql_type = BigInt)]
    pub ingresos: i64,
    #[diesel(sql_type = BigInt)]
    pub pedidos: i64,
}

#[derive(Serialize, QueryableByName)]
pub struct EstadoConteo {
    #[diesel(sql_type = Text)]
    pub estatus: String,
    #[diesel(sql_type = BigInt)]
    pub total: i64,
    #[diesel(sql_type = BigInt)]
    pub monto: i64,
}

#[derive(Serialize, QueryableByName)]
pub struct TopProducto {
    #[diesel(sql_type = Nullable<diesel::sql_types::Uuid>)]
    pub producto_id: Option<uuid::Uuid>,
    #[diesel(sql_type = Text)]
    pub nombre: String,
    #[diesel(sql_type = BigInt)]
    pub unidades: i64,
    #[diesel(sql_type = BigInt)]
    pub ingresos: i64,
}

#[derive(QueryableByName)]
struct Resumen1 {
    #[diesel(sql_type = BigInt)]
    ingresos: i64,
    #[diesel(sql_type = BigInt)]
    pedidos: i64,
    #[diesel(sql_type = BigInt)]
    unidades: i64,
    #[diesel(sql_type = Double)]
    ticket: f64,
}

fn parse_date(s: Option<&str>, default: NaiveDate) -> NaiveDate {
    s.and_then(|d| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok()).unwrap_or(default)
}

pub fn metrics(conn: &mut PgConnection, q: RangoQuery) -> ApiResult<Metricas> {
    let hoy = chrono::Utc::now().date_naive();
    let hasta = parse_date(q.hasta.as_deref(), hoy);
    let desde = parse_date(q.desde.as_deref(), hasta - Duration::days(29));
    if desde > hasta {
        return Err(ApiError::bad_request("La fecha 'desde' no puede ser posterior a 'hasta'"));
    }
    if (hasta - desde).num_days() > 731 {
        return Err(ApiError::bad_request("El rango máximo es de 2 años"));
    }
    let ini: NaiveDateTime = desde.and_hms_opt(0, 0, 0).unwrap();
    let fin: NaiveDateTime = (hasta + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap(); // exclusivo

    // Resumen de ventas concretadas
    let r: Resumen1 = diesel::sql_query(format!(
        "SELECT COALESCE(SUM(total),0)::bigint AS ingresos, COUNT(*)::bigint AS pedidos, \
         COALESCE((SELECT SUM(d.cantidad) FROM detalle_orden d JOIN ordenes o2 ON o2.id=d.orden_id \
            WHERE o2.estatus IN {VENDIDOS} AND o2.created_at >= $1 AND o2.created_at < $2),0)::bigint AS unidades, \
         COALESCE(AVG(total),0)::float8 AS ticket \
         FROM ordenes WHERE estatus IN {VENDIDOS} AND created_at >= $1 AND created_at < $2"))
        .bind::<Timestamp, _>(ini).bind::<Timestamp, _>(fin)
        .get_result(conn)?;

    #[derive(QueryableByName)]
    struct Dos { #[diesel(sql_type = BigInt)] c: i64, #[diesel(sql_type = BigInt)] m: i64 }
    let pend: Dos = diesel::sql_query(
        "SELECT COUNT(*)::bigint AS c, COALESCE(SUM(total),0)::bigint AS m FROM ordenes \
         WHERE estatus='pendiente' AND created_at >= $1 AND created_at < $2")
        .bind::<Timestamp, _>(ini).bind::<Timestamp, _>(fin).get_result(conn)?;

    #[derive(QueryableByName)]
    struct Uno { #[diesel(sql_type = BigInt)] n: i64 }
    let nuevos: Uno = diesel::sql_query(
        "SELECT COUNT(*)::bigint AS n FROM users WHERE created_at >= $1 AND created_at < $2")
        .bind::<Timestamp, _>(ini).bind::<Timestamp, _>(fin).get_result(conn)?;

    let por_dia: Vec<PuntoDia> = diesel::sql_query(format!(
        "SELECT created_at::date AS fecha, COALESCE(SUM(total),0)::bigint AS ingresos, COUNT(*)::bigint AS pedidos \
         FROM ordenes WHERE estatus IN {VENDIDOS} AND created_at >= $1 AND created_at < $2 \
         GROUP BY 1 ORDER BY 1"))
        .bind::<Timestamp, _>(ini).bind::<Timestamp, _>(fin).load(conn)?;

    let por_estado: Vec<EstadoConteo> = diesel::sql_query(
        "SELECT estatus, COUNT(*)::bigint AS total, COALESCE(SUM(total),0)::bigint AS monto FROM ordenes \
         WHERE created_at >= $1 AND created_at < $2 GROUP BY estatus ORDER BY total DESC")
        .bind::<Timestamp, _>(ini).bind::<Timestamp, _>(fin).load(conn)?;

    let top_productos: Vec<TopProducto> = diesel::sql_query(format!(
        "SELECT d.producto_id, MAX(d.nombre_producto) AS nombre, SUM(d.cantidad)::bigint AS unidades, \
         SUM(d.cantidad * d.precio_unitario)::bigint AS ingresos \
         FROM detalle_orden d JOIN ordenes o ON o.id=d.orden_id \
         WHERE o.estatus IN {VENDIDOS} AND o.created_at >= $1 AND o.created_at < $2 \
         GROUP BY d.producto_id ORDER BY ingresos DESC LIMIT 10"))
        .bind::<Timestamp, _>(ini).bind::<Timestamp, _>(fin).load(conn)?;

    Ok(Metricas {
        desde, hasta,
        resumen: Resumen {
            ingresos: r.ingresos,
            pedidos: r.pedidos,
            ticket_promedio: r.ticket.round() as i64,
            unidades: r.unidades,
            pendientes: pend.c,
            ingresos_pendientes: pend.m,
            clientes_nuevos: nuevos.n,
        },
        por_dia, por_estado, top_productos,
    })
}
