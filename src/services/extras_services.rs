//! Reseñas, wishlist, direcciones y cupones.
use crate::auth::AuthUser;
use crate::errors::{ApiError, ApiResult, OrNotFound};
use crate::models::extras::*;
use crate::models::orden::EstatusOrden;
use crate::models::products::Producto;
use crate::schema::{cupones, detalle_orden, direcciones, ordenes, productos, resenas, wishlist};
use crate::services::common::*;
use crate::services::product_services::get_product;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ================= Reseñas =================

#[derive(Deserialize)]
pub struct ResenaRequest {
    pub calificacion: i32,
    pub comentario: Option<String>,
}

fn validate_rating(c: i32) -> ApiResult<i32> {
    if !(1..=5).contains(&c) {
        return Err(ApiError::bad_request("La calificación debe estar entre 1 y 5"));
    }
    Ok(c)
}

pub fn list_reviews(conn: &mut PgConnection, producto_id: Uuid, p: Pagination) -> ApiResult<Paginated<Resena>> {
    let total: i64 = resenas::table.filter(resenas::producto_id.eq(producto_id)).count().get_result(conn)?;
    let items = resenas::table
        .filter(resenas::producto_id.eq(producto_id))
        .order(resenas::created_at.desc())
        .limit(p.per_page())
        .offset(p.offset())
        .select(Resena::as_select())
        .load(conn)?;
    Ok(Paginated::new(items, &p, total))
}

/// Sólo puede reseñar quien compró el producto (orden pagada, enviada o entregada).
pub fn create_review(conn: &mut PgConnection, producto_id: Uuid, user: AuthUser, req: ResenaRequest) -> ApiResult<Resena> {
    get_product(conn, producto_id)?;
    let compras: i64 = detalle_orden::table
        .inner_join(ordenes::table)
        .filter(ordenes::user_id.eq(user.id))
        .filter(detalle_orden::producto_id.eq(producto_id))
        .filter(ordenes::estatus.eq_any([
            EstatusOrden::Pagado.as_str(),
            EstatusOrden::Enviado.as_str(),
            EstatusOrden::Entregado.as_str(),
        ]))
        .count()
        .get_result(conn)?;
    if compras == 0 {
        return Err(ApiError::forbidden("Sólo puedes reseñar productos que hayas comprado"));
    }
    diesel::insert_into(resenas::table)
        .values(NewResena {
            user_id: Some(user.id),
            producto_id: Some(producto_id),
            calificacion: validate_rating(req.calificacion)?,
            comentario: optional_text("comentario", req.comentario, 5000)?,
        })
        .returning(Resena::as_returning())
        .get_result(conn)
        .map_err(|e| match ApiError::from(e) {
            ApiError::Conflict(_) => ApiError::conflict("Ya reseñaste este producto; puedes editar tu reseña"),
            other => other,
        })
}

fn get_review(conn: &mut PgConnection, id: Uuid) -> ApiResult<Resena> {
    resenas::table.find(id).select(Resena::as_select()).first(conn).or_not_found("Reseña")
}

pub fn update_review(conn: &mut PgConnection, id: Uuid, user: AuthUser, data: ResenaUpdate) -> ApiResult<Resena> {
    let r = get_review(conn, id)?;
    if r.user_id != Some(user.id) {
        return Err(ApiError::forbidden("Sólo puedes editar tus propias reseñas"));
    }
    if let Some(c) = data.calificacion {
        validate_rating(c)?;
    }
    if data.calificacion.is_none() && data.comentario.is_none() {
        return Ok(r);
    }
    Ok(diesel::update(resenas::table.find(id))
        .set(&data)
        .returning(Resena::as_returning())
        .get_result(conn)?)
}

pub fn delete_review(conn: &mut PgConnection, id: Uuid, user: AuthUser) -> ApiResult<()> {
    let r = get_review(conn, id)?;
    user.require_owner_or_admin(r.user_id)?;
    diesel::delete(resenas::table.find(id)).execute(conn)?;
    Ok(())
}

// ================= Wishlist =================

#[derive(Deserialize)]
pub struct WishlistRequest {
    pub producto_id: Uuid,
}

#[derive(Serialize)]
pub struct WishlistEntry {
    pub id: Uuid,
    pub agregado: chrono::NaiveDateTime,
    pub producto: Producto,
}

pub fn wishlist_of(conn: &mut PgConnection, user_id: Uuid) -> ApiResult<Vec<WishlistEntry>> {
    let rows: Vec<(WishlistItem, Producto)> = wishlist::table
        .inner_join(productos::table)
        .filter(wishlist::user_id.eq(user_id))
        .order(wishlist::created_at.desc())
        .select((WishlistItem::as_select(), Producto::as_select()))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(w, producto)| WishlistEntry { id: w.id, agregado: w.created_at, producto })
        .collect())
}

pub fn wishlist_add(conn: &mut PgConnection, user_id: Uuid, producto_id: Uuid) -> ApiResult<Vec<WishlistEntry>> {
    get_product(conn, producto_id)?;
    // Idempotente: si ya estaba, no pasa nada
    diesel::insert_into(wishlist::table)
        .values((wishlist::user_id.eq(user_id), wishlist::producto_id.eq(producto_id)))
        .on_conflict((wishlist::user_id, wishlist::producto_id))
        .do_nothing()
        .execute(conn)?;
    wishlist_of(conn, user_id)
}

pub fn wishlist_remove(conn: &mut PgConnection, user_id: Uuid, producto_id: Uuid) -> ApiResult<()> {
    let n = diesel::delete(
        wishlist::table.filter(wishlist::user_id.eq(user_id).and(wishlist::producto_id.eq(producto_id))),
    )
    .execute(conn)?;
    if n == 0 {
        return Err(ApiError::not_found("El producto no está en tu wishlist"));
    }
    Ok(())
}

// ================= Direcciones =================

fn clean_address(mut d: DireccionData, user_id: Uuid) -> ApiResult<DireccionData> {
    d.user_id = user_id;
    d.direccion = required_text("direccion", &d.direccion, 2000)?;
    d.titulo = optional_text("titulo", d.titulo, 100)?;
    d.ciudad = optional_text("ciudad", d.ciudad, 100)?;
    d.estado = optional_text("estado", d.estado, 100)?;
    d.codigo_postal = optional_text("codigo_postal", d.codigo_postal, 20)?;
    d.pais = optional_text("pais", d.pais, 100)?;
    d.telefono = optional_text("telefono", d.telefono, 50)?;
    Ok(d)
}

pub fn addresses_of(conn: &mut PgConnection, user_id: Uuid) -> ApiResult<Vec<Direccion>> {
    Ok(direcciones::table
        .filter(direcciones::user_id.eq(user_id))
        .order(direcciones::created_at.asc())
        .select(Direccion::as_select())
        .load(conn)?)
}

pub fn address_create(conn: &mut PgConnection, user_id: Uuid, data: DireccionData) -> ApiResult<Direccion> {
    let count: i64 = direcciones::table.filter(direcciones::user_id.eq(user_id)).count().get_result(conn)?;
    if count >= 20 {
        return Err(ApiError::bad_request("Máximo 20 direcciones por usuario"));
    }
    Ok(diesel::insert_into(direcciones::table)
        .values(clean_address(data, user_id)?)
        .returning(Direccion::as_returning())
        .get_result(conn)?)
}

/// PUT reemplaza la dirección completa.
pub fn address_update(conn: &mut PgConnection, user_id: Uuid, id: Uuid, data: DireccionData) -> ApiResult<Direccion> {
    diesel::update(direcciones::table.filter(direcciones::id.eq(id).and(direcciones::user_id.eq(user_id))))
        .set(clean_address(data, user_id)?)
        .returning(Direccion::as_returning())
        .get_result(conn)
        .or_not_found("Dirección")
}

pub fn address_delete(conn: &mut PgConnection, user_id: Uuid, id: Uuid) -> ApiResult<()> {
    let n = diesel::delete(direcciones::table.filter(direcciones::id.eq(id).and(direcciones::user_id.eq(user_id))))
        .execute(conn)?;
    if n == 0 {
        return Err(ApiError::not_found("Dirección no encontrada"));
    }
    Ok(())
}

// ================= Cupones =================

#[derive(Serialize)]
pub struct CuponValidacion {
    pub codigo: String,
    pub valido: bool,
    pub mensaje: Option<String>,
    pub subtotal: i64,
    pub descuento: i64,
    pub total: i64,
}

fn validate_coupon_values(pct: Option<i32>, fijo: Option<i64>) -> ApiResult<()> {
    match (pct, fijo) {
        (Some(p), None) if (1..=100).contains(&p) => Ok(()),
        (None, Some(f)) if f > 0 => Ok(()),
        _ => Err(ApiError::bad_request(
            "Indica sólo uno: 'descuento_porcentaje' (1-100) o 'descuento_fijo' (> 0)",
        )),
    }
}

pub fn coupon_list(conn: &mut PgConnection, p: Pagination) -> ApiResult<Paginated<Cupon>> {
    let total: i64 = cupones::table.count().get_result(conn)?;
    let items = cupones::table
        .order(cupones::created_at.desc())
        .limit(p.per_page())
        .offset(p.offset())
        .select(Cupon::as_select())
        .load(conn)?;
    Ok(Paginated::new(items, &p, total))
}

pub fn coupon_create(conn: &mut PgConnection, mut data: NewCupon) -> ApiResult<Cupon> {
    data.codigo = required_text("codigo", &data.codigo, 100)?.to_uppercase();
    validate_coupon_values(data.descuento_porcentaje, data.descuento_fijo)?;
    if matches!(data.uso_maximo, Some(m) if m < 1) {
        return Err(ApiError::bad_request("uso_maximo debe ser al menos 1"));
    }
    diesel::insert_into(cupones::table)
        .values(&data)
        .returning(Cupon::as_returning())
        .get_result(conn)
        .map_err(|e| match ApiError::from(e) {
            ApiError::Conflict(_) => ApiError::conflict("Ya existe un cupón con ese código"),
            other => other,
        })
}

pub fn coupon_update(conn: &mut PgConnection, id: Uuid, data: CuponUpdate) -> ApiResult<Cupon> {
    cupones::table.find(id).select(cupones::id).first::<Uuid>(conn).or_not_found("Cupón")?;
    // Si cambian el tipo de descuento, el resultado debe seguir teniendo exactamente uno
    if data.descuento_porcentaje.is_some() || data.descuento_fijo.is_some() {
        validate_coupon_values(data.descuento_porcentaje, data.descuento_fijo)?;
        diesel::update(cupones::table.find(id))
            .set((
                cupones::descuento_porcentaje.eq(data.descuento_porcentaje),
                cupones::descuento_fijo.eq(data.descuento_fijo),
            ))
            .execute(conn)?;
    }
    if data.fecha_expiracion.is_some() || data.uso_maximo.is_some() || data.activo.is_some() {
        diesel::update(cupones::table.find(id))
            .set(&CuponUpdate { descuento_porcentaje: None, descuento_fijo: None, ..data })
            .execute(conn)?;
    }
    Ok(cupones::table.find(id).select(Cupon::as_select()).first(conn)?)
}

pub fn coupon_delete(conn: &mut PgConnection, id: Uuid) -> ApiResult<()> {
    let n = diesel::delete(cupones::table.find(id)).execute(conn)?;
    if n == 0 {
        return Err(ApiError::not_found("Cupón no encontrado"));
    }
    Ok(())
}

/// Calcula cuánto descontaría el cupón sobre el carrito actual del usuario.
pub fn coupon_check(conn: &mut PgConnection, user_id: Uuid, codigo: &str) -> ApiResult<CuponValidacion> {
    let cupon = crate::services::ordenes_services::find_coupon(conn, codigo, false)?;
    let subtotal = crate::services::cart_services::view(conn, user_id)?.total;
    Ok(match cupon.descuento_para(subtotal, now()) {
        Ok(descuento) => CuponValidacion {
            codigo: cupon.codigo, valido: true, mensaje: None, subtotal, descuento, total: subtotal - descuento,
        },
        Err(msg) => CuponValidacion {
            codigo: cupon.codigo, valido: false, mensaje: Some(msg), subtotal, descuento: 0, total: subtotal,
        },
    })
}
