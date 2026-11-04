//! Integración de JarStore con **MeliHub** (hub de Mercado Libre):
//!   - `importar`: trae el catálogo de MeliHub y lo refleja en `productos`
//!     (crea/actualiza, vinculando por `melihub_id`). El precio es el del canal
//!     JarStore que sirve MeliHub.
//!   - `melihub_ids_de`: para una venta, devuelve qué productos vendidos tienen
//!     origen en MeliHub, para reportar la baja de stock (ver `melihub.rs`).
//!
//! La re-importación NO toca el slug ni las imágenes ya existentes (sólo nombre,
//! precio, stock y estado), para no pisar ajustes hechos en JarStore.

use crate::errors::ApiResult;
use crate::melihub::{Melihub, PublicProducto};
use crate::models::products::{NewProductImage, NewProducto};
use crate::schema::{product_images, producto_slugs_antiguos, productos};
use crate::services::common::{slugify, unique_slug};
use chrono::Utc;
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize, Debug, Default)]
pub struct SyncResult {
    pub creados: i64,
    pub actualizados: i64,
    pub errores: i64,
    pub total: i64,
}

fn now() -> chrono::NaiveDateTime {
    Utc::now().naive_utc()
}

/// ¿El slug ya está ocupado por un producto o por un slug histórico (301)?
fn slug_ocupado(conn: &mut PgConnection, s: &str) -> ApiResult<bool> {
    let en_productos: i64 = productos::table.filter(productos::slug.eq(s)).count().get_result(conn)?;
    if en_productos > 0 {
        return Ok(true);
    }
    let en_historicos: i64 = producto_slugs_antiguos::table
        .filter(producto_slugs_antiguos::slug.eq(s))
        .count()
        .get_result(conn)?;
    Ok(en_historicos > 0)
}

/// Mercado Libre suele dar thumbnails en http; forzamos https y pedimos mayor resolución.
fn normalizar_imagen(url: &str) -> String {
    let https = if let Some(rest) = url.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        url.to_string()
    };
    // En las URLs de ML, "-I.jpg" es el thumbnail; "-O.jpg" es más grande.
    https.replace("-I.jpg", "-O.jpg").replace("-I.webp", "-O.webp")
}

/// Importa (crea o actualiza) en JarStore el catálogo publicado por MeliHub.
/// `owner` es el admin que dispara la sincronización (dueño de los productos nuevos).
pub fn importar(conn: &mut PgConnection, client: &Melihub, owner: Uuid) -> ApiResult<SyncResult> {
    let items = client.fetch_productos()?;
    let mut r = SyncResult { total: items.len() as i64, ..Default::default() };

    for it in items {
        match upsert_uno(conn, owner, &it) {
            Ok(true) => r.creados += 1,
            Ok(false) => r.actualizados += 1,
            Err(e) => {
                r.errores += 1;
                log::warn!("MeliHub: no se pudo importar '{}' ({}): {e}", it.titulo, it.ml_item_id);
            }
        }
    }
    Ok(r)
}

/// Crea o actualiza un producto. Devuelve `true` si lo creó, `false` si lo actualizó.
/// Cada ítem va en su propia transacción: un error no aborta toda la importación.
fn upsert_uno(conn: &mut PgConnection, owner: Uuid, it: &PublicProducto) -> ApiResult<bool> {
    conn.transaction(|conn| {
        let estatus = it.estado.eq_ignore_ascii_case("active");
        let stock = it.stock.max(0);
        let precio = it.precio.max(0);
        let descripcion = it.descripcion.clone().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let especificaciones = if it.especificaciones.is_array() {
            it.especificaciones.clone()
        } else {
            serde_json::json!([])
        };

        let existente: Option<Uuid> = productos::table
            .filter(productos::melihub_id.eq(it.id))
            .select(productos::id)
            .first(conn)
            .optional()?;

        if let Some(pid) = existente {
            // Re-sync: sólo datos "vivos". No tocamos slug ni imágenes.
            diesel::update(productos::table.find(pid))
                .set((
                    productos::nombre.eq(&it.titulo),
                    productos::precio.eq(precio),
                    productos::stock.eq(stock),
                    productos::estatus.eq(estatus),
                    productos::descripcion.eq(&descripcion),
                    productos::especificaciones.eq(&especificaciones),
                    productos::updated_at.eq(now()),
                ))
                .execute(conn)?;
            return Ok(false);
        }

        let slug = unique_slug(&slugify(&it.titulo), 160, |s| slug_ocupado(conn, s))?;
        let producto: crate::models::products::Producto = diesel::insert_into(productos::table)
            .values(NewProducto {
                nombre: it.titulo.chars().take(255).collect(),
                descripcion: descripcion.clone(),
                precio,
                stock,
                sku: Some(it.ml_item_id.clone()),
                estatus,
                categoria_id: None,
                user_id: Some(owner),
                marca: None,
                precio_anterior: None,
                destacado: false,
                especificaciones: especificaciones.clone(),
                compatibilidad: None,
                garantia_meses: None,
                slug,
                peso_gramos: None,
                largo_cm: None,
                ancho_cm: None,
                alto_cm: None,
                melihub_id: Some(it.id),
            })
            .returning(crate::models::products::Producto::as_returning())
            .get_result(conn)?;

        if let Some(url) = it.thumbnail.as_deref().filter(|u| !u.trim().is_empty()) {
            let url = normalizar_imagen(url);
            if url.len() <= 500 {
                diesel::insert_into(product_images::table)
                    .values(NewProductImage {
                        product_id: producto.id,
                        image_url: url,
                        alt_text: Some(it.titulo.chars().take(255).collect()),
                        is_main: true,
                        variantes: serde_json::json!([]),
                    })
                    .execute(conn)?;
            }
        }
        Ok(true)
    })
}

/// Para los productos vendidos (producto_id, cantidad), devuelve los que tienen
/// origen en MeliHub como (melihub_id, cantidad), listos para reportar la venta.
pub fn melihub_ids_de(conn: &mut PgConnection, lineas: &[(Uuid, i32)]) -> ApiResult<Vec<(Uuid, i32)>> {
    if lineas.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<Uuid> = lineas.iter().map(|(pid, _)| *pid).collect();
    let mapa: Vec<(Uuid, Option<Uuid>)> = productos::table
        .filter(productos::id.eq_any(&ids))
        .filter(productos::melihub_id.is_not_null())
        .select((productos::id, productos::melihub_id))
        .load(conn)?;

    let mut out = Vec::new();
    for (pid, qty) in lineas {
        if let Some((_, Some(mid))) = mapa.iter().find(|(p, _)| p == pid) {
            out.push((*mid, *qty));
        }
    }
    Ok(out)
}
