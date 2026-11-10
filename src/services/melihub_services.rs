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
use crate::melihub::{Melihub, PublicProducto, VariantePub};
use crate::models::products::{NewProductImage, NewProducto, NewVariante, NewVariantImage};
use crate::schema::{product_images, product_variant_images, product_variants, producto_slugs_antiguos, productos};
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
    /// Productos de MeliHub que se pausaron por ya no venir en la sincronización
    /// (duplicados de catálogo descartados, o despublicados en Mercado Libre).
    pub pausados: i64,
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
    let servidos: Vec<uuid::Uuid> = items.iter().map(|i| i.id).collect();
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

    // Pausa los productos vinculados a MeliHub que ya NO vienen en la respuesta
    // (duplicados de catálogo descartados o publicaciones cerradas en ML). No se
    // borran, por si el admin quiere revisarlos; quedan en "Pausados".
    if !servidos.is_empty() {
        r.pausados = diesel::update(
            productos::table
                .filter(productos::melihub_id.is_not_null())
                .filter(productos::melihub_id.ne_all(servidos))
                .filter(productos::estatus.eq(true)),
        )
        .set(productos::estatus.eq(false))
        .execute(conn)? as i64;
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
        // Crea (o reutiliza) la categoría por nombre y la asigna al producto.
        let categoria_id = match it.categoria.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(nombre) => Some(crate::services::category_services::find_or_create(conn, nombre)?.id),
            None => None,
        };

        let existente: Option<Uuid> = productos::table
            .filter(productos::melihub_id.eq(it.id))
            .select(productos::id)
            .first(conn)
            .optional()?;

        if let Some(pid) = existente {
            // Re-sync: datos "vivos" + galería y variantes (vienen de ML). No tocamos el slug.
            diesel::update(productos::table.find(pid))
                .set((
                    productos::nombre.eq(&it.titulo),
                    productos::precio.eq(precio),
                    productos::stock.eq(stock),
                    productos::estatus.eq(estatus),
                    productos::descripcion.eq(&descripcion),
                    productos::especificaciones.eq(&especificaciones),
                    productos::categoria_id.eq(categoria_id),
                    productos::video_url.eq(&it.video),
                    productos::updated_at.eq(now()),
                ))
                .execute(conn)?;
            sincronizar_imagenes(conn, pid, &it.titulo, it)?;
            sincronizar_variantes(conn, pid, precio, &it.variantes)?;
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
                categoria_id,
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
                video_url: it.video.clone(),
            })
            .returning(crate::models::products::Producto::as_returning())
            .get_result(conn)?;

        sincronizar_imagenes(conn, producto.id, &it.titulo, it)?;
        sincronizar_variantes(conn, producto.id, precio, &it.variantes)?;
        Ok(true)
    })
}

/// Reemplaza la galería del producto con TODAS las imágenes que llegan de MeliHub (la
/// primera = principal). Si no hay galería, cae al thumbnail. Las imágenes no las
/// referencia ningún pedido, así que es seguro borrarlas y re-crearlas en cada sync.
fn sincronizar_imagenes(conn: &mut PgConnection, product_id: Uuid, titulo: &str, it: &PublicProducto) -> ApiResult<()> {
    let mut urls: Vec<String> = it
        .imagenes
        .iter()
        .map(|u| normalizar_imagen(u))
        .filter(|u| !u.is_empty() && u.len() <= 500)
        .collect();
    if urls.is_empty() {
        if let Some(t) = it.thumbnail.as_deref().filter(|u| !u.trim().is_empty()) {
            let t = normalizar_imagen(t);
            if t.len() <= 500 {
                urls.push(t);
            }
        }
    }
    if urls.is_empty() {
        return Ok(());
    }
    diesel::delete(product_images::table.filter(product_images::product_id.eq(product_id))).execute(conn)?;
    let alt: String = titulo.chars().take(255).collect();
    for (i, url) in urls.into_iter().enumerate() {
        diesel::insert_into(product_images::table)
            .values(NewProductImage {
                product_id,
                image_url: url,
                alt_text: Some(alt.clone()),
                is_main: i == 0,
                variantes: serde_json::json!([]),
            })
            .execute(conn)?;
    }
    Ok(())
}

/// Crea o actualiza las variantes desde MeliHub SIN borrar las existentes (los pedidos
/// las referencian por id). Empareja por (nombre, valor): refresca stock y precio, y
/// reemplaza las imágenes de la variante. Los atributos múltiples de ML (p. ej. Color +
/// Talla) se aplanan a un solo par nombre/valor ("Color / Talla" = "Rojo / M").
fn sincronizar_variantes(conn: &mut PgConnection, product_id: Uuid, precio_base: i64, variantes: &[VariantePub]) -> ApiResult<()> {
    for v in variantes {
        let nombre: String = v
            .atributos
            .iter()
            .map(|a| a.nombre.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" / ");
        let valor: String = v
            .atributos
            .iter()
            .map(|a| a.valor.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" / ");
        let nombre: String = if nombre.is_empty() { "Variante".into() } else { nombre.chars().take(255).collect() };
        let valor: String = if valor.is_empty() { format!("#{}", v.id) } else { valor.chars().take(255).collect() };
        let extra = (v.precio - precio_base).max(0);
        let stock = v.stock.max(0);

        let existente: Option<Uuid> = product_variants::table
            .filter(product_variants::producto_id.eq(product_id))
            .filter(product_variants::nombre.eq(&nombre))
            .filter(product_variants::valor.eq(&valor))
            .select(product_variants::id)
            .first(conn)
            .optional()?;
        let variant_id = match existente {
            Some(id) => {
                diesel::update(product_variants::table.find(id))
                    .set((
                        product_variants::stock.eq(stock),
                        product_variants::precio_extra.eq(extra),
                        product_variants::updated_at.eq(now()),
                    ))
                    .execute(conn)?;
                id
            }
            None => diesel::insert_into(product_variants::table)
                .values(NewVariante { producto_id: product_id, nombre, valor, stock, precio_extra: extra })
                .returning(product_variants::id)
                .get_result(conn)?,
        };
        // Imágenes propias de la variante (seguro borrar+recrear: no las referencian pedidos).
        diesel::delete(product_variant_images::table.filter(product_variant_images::variant_id.eq(variant_id))).execute(conn)?;
        for (i, url) in v
            .imagenes
            .iter()
            .map(|u| normalizar_imagen(u))
            .filter(|u| !u.is_empty() && u.len() <= 500)
            .enumerate()
        {
            diesel::insert_into(product_variant_images::table)
                .values(NewVariantImage { variant_id, image_url: url, alt_text: None, is_main: i == 0, variantes: serde_json::json!([]) })
                .execute(conn)?;
        }
    }
    Ok(())
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
