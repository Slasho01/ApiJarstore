use crate::auth::AuthUser;
use crate::errors::{ApiError, ApiResult, OrNotFound};
use crate::models::category::default_true;
use crate::models::products::*;
use crate::models::category::Categoria;
use crate::schema::{categorias, detalle_orden, ordenes, product_images, product_variant_images, product_variants, producto_complementos, producto_slugs_antiguos, productos, resenas};
use crate::services::category_services;
use crate::services::common::*;
use diesel::dsl::{count_star, sql};
use diesel::sql_types::{Text, BigInt};
use diesel::prelude::*;
use diesel::sql_types::{Double, Nullable};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ---------- Requests ----------

#[derive(Deserialize)]
pub struct NewVarianteRequest {
    pub nombre: String,
    pub valor: String,
    pub stock: i32,
    #[serde(default)]
    pub precio_extra: i64,
    #[serde(default)]
    pub imagenes: Vec<NewImageRequest>,
}

#[derive(Deserialize)]
pub struct NewImageRequest {
    pub image_url: String,
    pub alt_text: Option<String>,
    #[serde(default)]
    pub is_main: bool,
    /// Tamaños generados por /api/uploads (opcional)
    #[serde(default)]
    pub variantes: Vec<ImagenVariante>,
}

fn image_variants(v: Vec<ImagenVariante>) -> ApiResult<serde_json::Value> {
    if v.len() > 12 {
        return Err(ApiError::bad_request("Demasiadas variantes de imagen"));
    }
    for x in &v {
        valid_url("variantes.url", &x.url)?;
    }
    serde_json::to_value(v).map_err(|e| ApiError::internal(e.to_string()))
}

#[derive(Deserialize)]
pub struct NewProductoRequest {
    pub nombre: String,
    pub descripcion: Option<String>,
    pub precio: i64,
    pub stock: i32,
    pub sku: Option<String>,
    #[serde(default = "default_true")]
    pub estatus: bool,
    pub categoria_id: Option<Uuid>,
    pub marca: Option<String>,
    pub precio_anterior: Option<i64>,
    #[serde(default)]
    pub destacado: bool,
    #[serde(default)]
    pub especificaciones: Vec<Especificacion>,
    pub compatibilidad: Option<String>,
    pub garantia_meses: Option<i32>,
    /// Peso y dimensiones para cotizar envíos
    pub peso_gramos: Option<i32>,
    pub largo_cm: Option<i32>,
    pub ancho_cm: Option<i32>,
    pub alto_cm: Option<i32>,
    /// Opcional: si no se envía se genera a partir del nombre
    pub slug: Option<String>,
    #[serde(default)]
    pub variantes: Vec<NewVarianteRequest>,
    #[serde(default)]
    pub imagenes: Vec<NewImageRequest>,
}

/// PUT /productos/{id}. Los campos ausentes no se tocan.
/// `precio_anterior` y `garantia_meses` aceptan `null` para quitarlos;
/// en los textos opcionales, una cadena vacía los deja en blanco.
#[derive(Deserialize)]
pub struct UpdateProductoRequest {
    #[serde(flatten)]
    pub data: ProductoUpdate,
    pub especificaciones: Option<Vec<Especificacion>>,
    #[serde(default, deserialize_with = "double_option")]
    pub precio_anterior: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub garantia_meses: Option<Option<i32>>,
}

#[derive(Deserialize, Default)]
pub struct ProductFilter {
    /// Búsqueda por nombre / descripción / sku
    pub q: Option<String>,
    pub categoria_id: Option<Uuid>,
    /// Categoría por slug o id (incluye sus subcategorías)
    pub categoria: Option<String>,
    pub vendedor_id: Option<Uuid>,
    /// Valoración promedio mínima (1-5)
    pub valoracion: Option<f64>,
    pub min_precio: Option<i64>,
    pub max_precio: Option<i64>,
    pub en_stock: Option<bool>,
    /// Una o varias marcas separadas por coma
    pub marca: Option<String>,
    /// Marca por su slug (URL /marca/{slug})
    pub marca_slug: Option<String>,
    /// Sólo productos con precio anterior (en oferta)
    pub oferta: Option<bool>,
    pub destacado: Option<bool>,
    /// Características en JSON: {"RAM": ["16 GB", "32 GB"], "Color": ["Negro"]}
    /// (OR dentro de una característica, AND entre características)
    pub specs: Option<String>,
    /// relevancia | precio_asc | precio_desc | nombre | recientes | descuento | mas_vendidos
    pub orden: Option<String>,
    #[serde(skip)]
    pub incluir_inactivos: bool,
}

// ---------- Responses ----------

#[derive(Serialize, Default, Clone, Copy)]
pub struct Calificacion {
    pub promedio: Option<f64>,
    pub total: i64,
}

#[derive(Serialize)]
pub struct ProductoListItem {
    #[serde(flatten)]
    pub producto: Producto,
    pub imagen_principal: Option<String>,
    /// Tamaños de la imagen principal (para srcset)
    pub imagen_variantes: serde_json::Value,
    pub calificacion: Calificacion,
    /// Nº de variantes: si es > 0 hay que elegir una antes de comprar
    pub variantes: i64,
}

#[derive(Serialize)]
pub struct VarianteDetalle {
    #[serde(flatten)]
    pub variante: Variante,
    pub imagenes: Vec<VariantImage>,
}

#[derive(Serialize)]
pub struct ProductoDetalle {
    #[serde(flatten)]
    pub producto: Producto,
    /// Categoría y sus ancestros, de la raíz a la hoja (breadcrumbs)
    pub categorias_ruta: Vec<Categoria>,
    pub variantes: Vec<VarianteDetalle>,
    pub imagenes: Vec<ProductImage>,
    pub calificacion: Calificacion,
}

// ---------- Helpers ----------

fn ratings_for(conn: &mut PgConnection, ids: &[Uuid]) -> ApiResult<HashMap<Uuid, Calificacion>> {
    let rows: Vec<(Option<Uuid>, Option<f64>, i64)> = resenas::table
        .filter(resenas::producto_id.eq_any(ids))
        .group_by(resenas::producto_id)
        .select((
            resenas::producto_id,
            sql::<Nullable<Double>>("ROUND(AVG(calificacion)::numeric, 2)::float8"),
            count_star(),
        ))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, promedio, total)| id.map(|id| (id, Calificacion { promedio, total })))
        .collect())
}

/// Imagen principal con sus tamaños generados.
pub fn main_images_full(conn: &mut PgConnection, ids: &[Uuid]) -> ApiResult<HashMap<Uuid, (String, serde_json::Value)>> {
    let rows: Vec<(Uuid, String, serde_json::Value)> = product_images::table
        .filter(product_images::product_id.eq_any(ids))
        .order((product_images::product_id, product_images::is_main.desc(), product_images::created_at.asc()))
        .distinct_on(product_images::product_id)
        .select((product_images::product_id, product_images::image_url, product_images::variantes))
        .load(conn)?;
    Ok(rows.into_iter().map(|(id, url, v)| (id, (url, v))).collect())
}

pub fn main_images_for(conn: &mut PgConnection, ids: &[Uuid]) -> ApiResult<HashMap<Uuid, String>> {
    // Imagen principal, o la más antigua si ninguna está marcada como principal
    let rows: Vec<(Uuid, String)> = product_images::table
        .filter(product_images::product_id.eq_any(ids))
        .order((product_images::product_id, product_images::is_main.desc(), product_images::created_at.asc()))
        .distinct_on(product_images::product_id)
        .select((product_images::product_id, product_images::image_url))
        .load(conn)?;
    Ok(rows.into_iter().collect())
}

pub fn get_product(conn: &mut PgConnection, id: Uuid) -> ApiResult<Producto> {
    productos::table.find(id).select(Producto::as_select()).first(conn).or_not_found("Producto")
}

/// Resuelve un producto por id, slug actual o slug antiguo (para redirecciones 301).
pub fn resolve_key(conn: &mut PgConnection, key: &str) -> ApiResult<Uuid> {
    if let Ok(id) = Uuid::parse_str(key) {
        return Ok(id);
    }
    if let Some(id) = productos::table.filter(productos::slug.eq(key)).select(productos::id).first::<Uuid>(conn).optional()? {
        return Ok(id);
    }
    producto_slugs_antiguos::table
        .find(key)
        .select(producto_slugs_antiguos::producto_id)
        .first::<Uuid>(conn)
        .or_not_found("Producto")
}

fn slug_taken(conn: &mut PgConnection, slug: &str, except: Option<Uuid>) -> ApiResult<bool> {
    let mut q = productos::table.filter(productos::slug.eq(slug)).into_boxed();
    if let Some(id) = except {
        q = q.filter(productos::id.ne(id));
    }
    let old: bool = diesel::select(diesel::dsl::exists(
        producto_slugs_antiguos::table.filter(producto_slugs_antiguos::slug.eq(slug))
            .filter(producto_slugs_antiguos::producto_id.ne(except.unwrap_or_default())),
    )).get_result(conn)?;
    Ok(old || diesel::select(diesel::dsl::exists(q)).get_result(conn)?)
}

/// Carga el producto y verifica que el usuario sea su dueño (o admin).
pub fn get_owned_product(conn: &mut PgConnection, id: Uuid, user: &AuthUser) -> ApiResult<Producto> {
    let p = get_product(conn, id)?;
    user.require_owner_or_admin(p.user_id)?;
    Ok(p)
}

pub fn get_variant(conn: &mut PgConnection, id: Uuid) -> ApiResult<Variante> {
    product_variants::table.find(id).select(Variante::as_select()).first(conn).or_not_found("Variante")
}

fn get_owned_variant(conn: &mut PgConnection, id: Uuid, user: &AuthUser) -> ApiResult<Variante> {
    let v = get_variant(conn, id)?;
    get_owned_product(conn, v.producto_id, user)?;
    Ok(v)
}

fn validate_variant(v: &NewVarianteRequest) -> ApiResult<(String, String)> {
    non_negative("stock", v.stock)?;
    non_negative("precio_extra", v.precio_extra)?;
    Ok((required_text("nombre", &v.nombre, 255)?, required_text("valor", &v.valor, 255)?))
}

fn insert_product_image(conn: &mut PgConnection, product_id: Uuid, img: NewImageRequest) -> ApiResult<ProductImage> {
    if img.is_main {
        diesel::update(product_images::table.filter(product_images::product_id.eq(product_id)))
            .set(product_images::is_main.eq(false))
            .execute(conn)?;
    }
    Ok(diesel::insert_into(product_images::table)
        .values(NewProductImage {
            product_id,
            image_url: valid_url("image_url", &img.image_url)?,
            alt_text: optional_text("alt_text", img.alt_text, 255)?,
            is_main: img.is_main,
            variantes: image_variants(img.variantes)?,
        })
        .returning(ProductImage::as_returning())
        .get_result(conn)?)
}

fn insert_variant_image(conn: &mut PgConnection, variant_id: Uuid, img: NewImageRequest) -> ApiResult<VariantImage> {
    if img.is_main {
        diesel::update(product_variant_images::table.filter(product_variant_images::variant_id.eq(variant_id)))
            .set(product_variant_images::is_main.eq(false))
            .execute(conn)?;
    }
    Ok(diesel::insert_into(product_variant_images::table)
        .values(NewVariantImage {
            variant_id,
            image_url: valid_url("image_url", &img.image_url)?,
            alt_text: optional_text("alt_text", img.alt_text, 255)?,
            is_main: img.is_main,
            variantes: image_variants(img.variantes)?,
        })
        .returning(VariantImage::as_returning())
        .get_result(conn)?)
}

fn insert_variant(conn: &mut PgConnection, producto_id: Uuid, v: NewVarianteRequest) -> ApiResult<Variante> {
    let (nombre, valor) = validate_variant(&v)?;
    let variante: Variante = diesel::insert_into(product_variants::table)
        .values(NewVariante { producto_id, nombre, valor, stock: v.stock, precio_extra: v.precio_extra })
        .returning(Variante::as_returning())
        .get_result(conn)?;
    for img in v.imagenes {
        insert_variant_image(conn, variante.id, img)?;
    }
    Ok(variante)
}

// ---------- Productos ----------

fn parse_specs(raw: Option<&str>) -> ApiResult<Vec<(String, Vec<String>)>> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else { return Ok(vec![]) };
    let map: std::collections::BTreeMap<String, Vec<String>> = serde_json::from_str(raw)
        .map_err(|_| ApiError::bad_request("'specs' debe ser JSON: {\"RAM\": [\"16 GB\"]}"))?;
    Ok(map.into_iter().filter(|(_, v)| !v.is_empty()).take(20).collect())
}

pub fn list(conn: &mut PgConnection, f: ProductFilter, p: Pagination) -> ApiResult<Paginated<ProductoListItem>> {
    let specs = parse_specs(f.specs.as_deref())?;
    let cat_ids: Option<Vec<Uuid>> = match (f.categoria.as_deref().filter(|c| !c.is_empty()), f.categoria_id) {
        (Some(key), _) => {
            let root = category_services::get(conn, key)?.id;
            Some(category_services::with_descendants(conn, root)?)
        }
        (None, Some(id)) => Some(category_services::with_descendants(conn, id)?),
        _ => None,
    };
    let q_text = f.q.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let marcas: Vec<String> = f.marca.as_deref().unwrap_or("").split(',')
        .map(|m| m.trim().to_lowercase()).filter(|m| !m.is_empty()).collect();
    let build = || {
        let mut q = productos::table.into_boxed();
        if !f.incluir_inactivos {
            q = q.filter(productos::estatus.eq(true));
        }
        if let Some(text) = &q_text {
            // Cada palabra debe aparecer en nombre, marca, sku o descripción
            for word in text.split_whitespace().take(8) {
                let pattern = format!("%{word}%");
                q = q.filter(
                    productos::nombre.ilike(pattern.clone())
                        .or(productos::marca.ilike(pattern.clone()))
                        .or(productos::sku.ilike(pattern.clone()))
                        .or(productos::descripcion.ilike(pattern)),
                );
            }
        }
        if let Some(ids) = &cat_ids {
            q = q.filter(productos::categoria_id.eq_any(ids.clone()));
        }
        if let Some(min) = f.valoracion.filter(|v| *v > 0.0) {
            q = q.filter(sql::<diesel::sql_types::Bool>("(SELECT AVG(r.calificacion) FROM resenas r WHERE r.producto_id = productos.id) >= ")
                .bind::<Double, _>(min.min(5.0)));
        }
        if let Some(v) = f.vendedor_id {
            q = q.filter(productos::user_id.eq(v));
        }
        if let Some(min) = f.min_precio {
            q = q.filter(productos::precio.ge(min));
        }
        if let Some(max) = f.max_precio {
            q = q.filter(productos::precio.le(max));
        }
        if f.en_stock == Some(true) {
            q = q.filter(productos::stock.gt(0));
        }
        if !marcas.is_empty() {
            q = q.filter(sql::<diesel::sql_types::Bool>("LOWER(productos.marca) = ANY(")
                .bind::<diesel::sql_types::Array<Text>, _>(marcas.clone())
                .sql(")"));
        }
        if let Some(ms) = f.marca_slug.as_deref().filter(|m| !m.is_empty()) {
            q = q.filter(sql::<diesel::sql_types::Bool>("jarstore_slugify(productos.marca) = ").bind::<Text, _>(ms.to_string()));
        }
        if f.oferta == Some(true) {
            q = q.filter(productos::precio_anterior.is_not_null());
        }
        if f.destacado == Some(true) {
            q = q.filter(productos::destacado.eq(true));
        }
        for (nombre, valores) in &specs {
            // especificaciones @> '[{"nombre": n, "valor": v}]' para algún v
            let mut cond: Box<dyn BoxableExpression<productos::table, diesel::pg::Pg, SqlType = diesel::sql_types::Bool>> =
                Box::new(diesel::dsl::sql::<diesel::sql_types::Bool>("FALSE"));
            for v in valores {
                let needle = serde_json::json!([{ "nombre": nombre, "valor": v }]);
                cond = Box::new(cond.or(productos::especificaciones.contains(needle)));
            }
            q = q.filter(cond);
        }
        q
    };

    let total: i64 = build().count().get_result(conn)?;
    let query = build().select(Producto::as_select()).limit(p.per_page()).offset(p.offset());
    let orden = f.orden.as_deref().unwrap_or(if q_text.is_some() { "relevancia" } else { "recientes" });
    let items: Vec<Producto> = match Some(orden) {
        Some("relevancia") => {
            // Coincidencia en el nombre (al inicio > contiene) > marca > resto; con stock primero
            let t = q_text.clone().unwrap_or_default();
            query.order((
                sql::<BigInt>("CASE WHEN productos.nombre ILIKE ").bind::<Text, _>(format!("{t}%"))
                    .sql(" THEN 0 WHEN productos.nombre ILIKE ").bind::<Text, _>(format!("%{t}%"))
                    .sql(" THEN 1 WHEN productos.marca ILIKE ").bind::<Text, _>(format!("%{t}%"))
                    .sql(" THEN 2 ELSE 3 END").asc(),
                productos::stock.gt(0).desc(),
                productos::destacado.desc(),
                productos::created_at.desc(),
            )).load(conn)?
        }
        Some("mas_vendidos") => query
            .order((sql::<BigInt>("(SELECT COALESCE(SUM(d.cantidad), 0) FROM detalle_orden d JOIN ordenes o ON o.id = d.orden_id \
                WHERE d.producto_id = productos.id AND o.estatus IN ('pagado', 'enviado', 'entregado'))").desc(), productos::created_at.desc()))
            .load(conn)?,
        Some("precio_asc") => query.order((productos::precio.asc(), productos::id)).load(conn)?,
        Some("precio_desc") => query.order((productos::precio.desc(), productos::id)).load(conn)?,
        Some("nombre") => query.order((productos::nombre.asc(), productos::id)).load(conn)?,
        None | Some("recientes") => query.order((productos::created_at.desc(), productos::id)).load(conn)?,
        Some("descuento") => query
            .order((sql::<BigInt>("COALESCE(productos.precio_anterior - productos.precio, 0) * 100 / GREATEST(productos.precio_anterior, 1)").desc(), productos::id))
            .load(conn)?,
        Some(other) => return Err(ApiError::bad_request(format!(
            "orden '{other}' inválido (usa relevancia, precio_asc, precio_desc, nombre, recientes, descuento o mas_vendidos)"))),
    };
    let items = to_list_items(conn, items)?;
    Ok(Paginated::new(items, &p, total))
}

fn to_list_items(conn: &mut PgConnection, items: Vec<Producto>) -> ApiResult<Vec<ProductoListItem>> {
    let ids: Vec<Uuid> = items.iter().map(|p| p.id).collect();
    let ratings = ratings_for(conn, &ids)?;
    let mut images = main_images_full(conn, &ids)?;
    let counts: HashMap<Uuid, i64> = product_variants::table
        .filter(product_variants::producto_id.eq_any(&ids))
        .group_by(product_variants::producto_id)
        .select((product_variants::producto_id, count_star()))
        .load::<(Uuid, i64)>(conn)?
        .into_iter()
        .collect();
    Ok(items
        .into_iter()
        .map(|producto| ProductoListItem {
            variantes: counts.get(&producto.id).copied().unwrap_or(0),
            imagen_variantes: images.get(&producto.id).map(|(_, v)| v.clone()).unwrap_or_else(|| serde_json::json!([])),
            imagen_principal: images.remove(&producto.id).map(|(u, _)| u),
            calificacion: ratings.get(&producto.id).copied().unwrap_or_default(),
            producto,
        })
        .collect())
}

/// Productos activos más vendidos (por unidades en órdenes pagadas, enviadas o entregadas).
pub fn best_sellers(conn: &mut PgConnection, limit: i64) -> ApiResult<Vec<ProductoListItem>> {
    let top: Vec<(Option<Uuid>, Option<i64>)> = detalle_orden::table
        .inner_join(ordenes::table)
        .inner_join(productos::table)
        .filter(ordenes::estatus.eq_any(["pagado", "enviado", "entregado"]))
        .filter(productos::estatus.eq(true))
        .group_by(detalle_orden::producto_id)
        .select((detalle_orden::producto_id, diesel::dsl::sum(detalle_orden::cantidad)))
        .order(diesel::dsl::sum(detalle_orden::cantidad).desc())
        .limit(limit.clamp(1, 24))
        .load(conn)?;
    let ids: Vec<Uuid> = top.iter().filter_map(|(id, _)| *id).collect();
    let mut by_id: HashMap<Uuid, Producto> = productos::table
        .filter(productos::id.eq_any(&ids))
        .select(Producto::as_select())
        .load(conn)?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let ordered = ids.iter().filter_map(|id| by_id.remove(id)).collect();
    to_list_items(conn, ordered)
}

/// Productos de la misma categoría (o, si no tiene, de la misma marca).
pub fn related(conn: &mut PgConnection, id: Uuid, limit: i64) -> ApiResult<Vec<ProductoListItem>> {
    let p = get_product(conn, id)?;
    let mut q = productos::table
        .filter(productos::estatus.eq(true))
        .filter(productos::id.ne(id))
        .into_boxed();
    match (p.categoria_id, p.marca.as_deref()) {
        (Some(c), _) => q = q.filter(productos::categoria_id.eq(c)),
        (None, Some(m)) => q = q.filter(productos::marca.eq(m.to_string())),
        (None, None) => return Ok(vec![]),
    }
    let items = q
        .order((productos::stock.gt(0).desc(), productos::created_at.desc()))
        .limit(limit.clamp(1, 24))
        .select(Producto::as_select())
        .load(conn)?;
    to_list_items(conn, items)
}

// ---------- Facetas (para los filtros del catálogo) ----------

#[derive(Serialize, QueryableByName)]
pub struct MarcaFaceta {
    #[diesel(sql_type = Text)]
    pub marca: String,
    #[diesel(sql_type = Text)]
    pub slug: String,
    #[diesel(sql_type = BigInt)]
    pub total: i64,
}

#[derive(QueryableByName)]
struct SpecRow {
    #[diesel(sql_type = Text)]
    nombre: String,
    #[diesel(sql_type = Text)]
    valor: String,
    #[diesel(sql_type = BigInt)]
    total: i64,
}

#[derive(QueryableByName, Serialize)]
pub struct RangoPrecio {
    #[diesel(sql_type = Nullable<BigInt>)]
    pub min: Option<i64>,
    #[diesel(sql_type = Nullable<BigInt>)]
    pub max: Option<i64>,
    #[diesel(sql_type = BigInt)]
    pub total: i64,
}

#[derive(Serialize)]
pub struct ValorFaceta {
    pub valor: String,
    pub total: i64,
}

#[derive(Serialize)]
pub struct CaracteristicaFaceta {
    pub nombre: String,
    pub valores: Vec<ValorFaceta>,
}

#[derive(Serialize)]
pub struct Facetas {
    pub subcategorias: Vec<SubcategoriaFaceta>,
    pub marcas: Vec<MarcaFaceta>,
    pub caracteristicas: Vec<CaracteristicaFaceta>,
    pub precio: RangoPrecio,
}

#[derive(Deserialize)]
pub struct FacetFilter {
    pub categoria_id: Option<Uuid>,
    /// slug o id (incluye subcategorías)
    pub categoria: Option<String>,
    pub q: Option<String>,
}

#[derive(Serialize, QueryableByName)]
pub struct SubcategoriaFaceta {
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    pub id: Uuid,
    #[diesel(sql_type = Text)]
    pub slug: String,
    #[diesel(sql_type = Text)]
    pub nombre: String,
    #[diesel(sql_type = BigInt)]
    pub total: i64,
}

/// Marcas, características y rango de precios de los productos activos
/// (opcionalmente de una categoría y/o búsqueda).
pub fn facets(conn: &mut PgConnection, f: FacetFilter) -> ApiResult<Facetas> {
    use diesel::sql_types::{Array, Uuid as SqlUuid};
    let q = f.q.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(|s| format!("%{s}%"));
    let root = match (f.categoria.as_deref().filter(|c| !c.is_empty()), f.categoria_id) {
        (Some(key), _) => Some(category_services::get(conn, key)?.id),
        (None, Some(id)) => Some(id),
        _ => None,
    };
    let ids: Option<Vec<Uuid>> = match root { Some(id) => Some(category_services::with_descendants(conn, id)?), None => None };
    const WHERE: &str = "p.estatus AND ($1::uuid[] IS NULL OR p.categoria_id = ANY($1)) \
        AND ($2::text IS NULL OR p.nombre ILIKE $2 OR p.marca ILIKE $2 OR p.descripcion ILIKE $2 OR p.sku ILIKE $2)";

    // Subcategorías directas (de la categoría elegida o raíces) con nº de productos, incluidos los de sus hijas
    let subcategorias = diesel::sql_query(format!(
        "WITH RECURSIVE arbol AS ( \
           SELECT c.id AS raiz, c.id FROM categorias c WHERE c.estatus AND c.parent_id IS NOT DISTINCT FROM $3 \
           UNION ALL SELECT a.raiz, h.id FROM categorias h JOIN arbol a ON h.parent_id = a.id WHERE h.estatus) \
         SELECT c.id, c.slug, c.nombre, COUNT(p.id) AS total FROM categorias c \
         JOIN arbol a ON a.raiz = c.id LEFT JOIN productos p ON p.categoria_id = a.id AND {WHERE} \
         GROUP BY c.id, c.slug, c.nombre, c.orden HAVING COUNT(p.id) > 0 ORDER BY c.orden, c.nombre"))
        .bind::<Nullable<Array<SqlUuid>>, _>(ids.clone())
        .bind::<Nullable<Text>, _>(q.clone())
        .bind::<Nullable<SqlUuid>, _>(root)
        .load::<SubcategoriaFaceta>(conn)?;

    let marcas = diesel::sql_query(format!(
        "SELECT MIN(p.marca) AS marca, jarstore_slugify(MIN(p.marca)) AS slug, COUNT(*) AS total FROM productos p \
         WHERE {WHERE} AND p.marca IS NOT NULL AND p.marca <> '' \
         GROUP BY LOWER(p.marca) ORDER BY total DESC, marca LIMIT 50"))
        .bind::<Nullable<Array<SqlUuid>>, _>(ids.clone())
        .bind::<Nullable<Text>, _>(q.clone())
        .load::<MarcaFaceta>(conn)?;

    let rows = diesel::sql_query(format!(
        "SELECT e->>'nombre' AS nombre, e->>'valor' AS valor, COUNT(*) AS total \
         FROM productos p, jsonb_array_elements(p.especificaciones) e \
         WHERE {WHERE} AND e->>'nombre' <> '' AND e->>'valor' <> '' \
         GROUP BY 1, 2 ORDER BY 1, total DESC, 2 LIMIT 500"))
        .bind::<Nullable<Array<SqlUuid>>, _>(ids.clone())
        .bind::<Nullable<Text>, _>(q.clone())
        .load::<SpecRow>(conn)?;
    let mut caracteristicas: Vec<CaracteristicaFaceta> = Vec::new();
    for r in rows {
        match caracteristicas.last_mut() {
            Some(c) if c.nombre == r.nombre => c.valores.push(ValorFaceta { valor: r.valor, total: r.total }),
            _ => caracteristicas.push(CaracteristicaFaceta { nombre: r.nombre, valores: vec![ValorFaceta { valor: r.valor, total: r.total }] }),
        }
    }
    // Sólo características que sirven para filtrar (2+ valores o compartidas por varios productos)
    caracteristicas.retain(|c| c.valores.len() > 1 || c.valores.iter().any(|v| v.total > 1));
    caracteristicas.truncate(15);

    let precio = diesel::sql_query(format!(
        "SELECT MIN(p.precio) AS min, MAX(p.precio) AS max, COUNT(*) AS total FROM productos p WHERE {WHERE}"))
        .bind::<Nullable<Array<SqlUuid>>, _>(ids.clone())
        .bind::<Nullable<Text>, _>(q)
        .get_result::<RangoPrecio>(conn)?;

    Ok(Facetas { subcategorias, marcas, caracteristicas, precio })
}

// ---------- Validación de campos comerciales ----------

fn validate_specs(specs: Vec<Especificacion>) -> ApiResult<serde_json::Value> {
    if specs.len() > 60 {
        return Err(ApiError::bad_request("Máximo 60 especificaciones por producto"));
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(specs.len());
    for s in specs {
        let nombre = required_text("nombre de la especificación", &s.nombre, 100)?;
        let valor = required_text("valor de la especificación", &s.valor, 255)?;
        if !seen.insert(nombre.to_lowercase()) {
            return Err(ApiError::bad_request(format!("Especificación repetida: '{nombre}'")));
        }
        out.push(Especificacion { nombre, valor });
    }
    serde_json::to_value(out).map_err(|e| ApiError::internal(e.to_string()))
}

fn validate_garantia(g: Option<i32>) -> ApiResult<Option<i32>> {
    match g {
        Some(m) if !(0..=240).contains(&m) => Err(ApiError::bad_request("La garantía debe estar entre 0 y 240 meses")),
        other => Ok(other),
    }
}

/// Valida peso/dimensiones (no negativos y dentro de un máximo razonable).
fn validate_medida(field: &str, v: Option<i32>, max: i32) -> ApiResult<Option<i32>> {
    match v {
        Some(n) if !(0..=max).contains(&n) => Err(ApiError::bad_request(format!("'{field}' debe estar entre 0 y {max}"))),
        other => Ok(other),
    }
}

/// Mensaje claro para la regla precio_anterior > precio.
fn map_product_err(e: diesel::result::Error) -> ApiError {
    match ApiError::from(e) {
        ApiError::BadRequest(m) if m.contains("productos_precio_anterior_check") =>
            ApiError::bad_request("El precio anterior debe ser mayor que el precio actual (o déjalo vacío si no hay oferta)"),
        ApiError::Conflict(m) if m.contains("productos_sku_unique") => ApiError::conflict("Ya existe un producto con ese SKU"),
        other => other,
    }
}

/// Detalle completo. Los productos inactivos sólo los ve su dueño o un admin.
pub fn detail(conn: &mut PgConnection, id: Uuid, viewer: Option<&AuthUser>) -> ApiResult<ProductoDetalle> {
    let producto = get_product(conn, id)?;
    let categorias_ruta = match producto.categoria_id {
        Some(cid) => {
            let cat = category_services::get(conn, &cid.to_string())?;
            let mut ruta = category_services::ancestors(conn, &cat)?;
            ruta.push(cat);
            ruta
        }
        None => vec![],
    };
    if !producto.estatus {
        let allowed = viewer.map(|u| u.require_owner_or_admin(producto.user_id).is_ok()).unwrap_or(false);
        if !allowed {
            return Err(ApiError::not_found("Producto no encontrado"));
        }
    }

    let variantes: Vec<Variante> = Variante::belonging_to(&producto)
        .select(Variante::as_select())
        .order(product_variants::created_at.asc())
        .load(conn)?;
    let variant_images: Vec<VariantImage> = VariantImage::belonging_to(&variantes)
        .select(VariantImage::as_select())
        .order((product_variant_images::is_main.desc(), product_variant_images::created_at.asc()))
        .load(conn)?;
    let variantes = variant_images
        .grouped_by(&variantes)
        .into_iter()
        .zip(variantes)
        .map(|(imagenes, variante)| VarianteDetalle { variante, imagenes })
        .collect();

    let imagenes = ProductImage::belonging_to(&producto)
        .select(ProductImage::as_select())
        .order((product_images::is_main.desc(), product_images::created_at.asc()))
        .load(conn)?;
    let calificacion = ratings_for(conn, &[id])?.remove(&id).unwrap_or_default();

    Ok(ProductoDetalle { producto, categorias_ruta, variantes, imagenes, calificacion })
}

pub fn create(conn: &mut PgConnection, req: NewProductoRequest, user: AuthUser) -> ApiResult<ProductoDetalle> {
    conn.transaction(|conn| {
        let producto: Producto = diesel::insert_into(productos::table)
            .values(NewProducto {
                nombre: required_text("nombre", &req.nombre, 255)?,
                descripcion: optional_text("descripcion", req.descripcion, 10_000)?,
                precio: non_negative("precio", req.precio)?,
                stock: non_negative("stock", req.stock)?,
                sku: optional_text("sku", req.sku, 255)?,
                estatus: req.estatus,
                categoria_id: req.categoria_id,
                user_id: Some(user.id), // el dueño siempre es quien lo crea
                marca: optional_text("marca", req.marca, 100)?,
                precio_anterior: req.precio_anterior,
                destacado: req.destacado,
                especificaciones: validate_specs(req.especificaciones)?,
                compatibilidad: optional_text("compatibilidad", req.compatibilidad, 10_000)?,
                garantia_meses: validate_garantia(req.garantia_meses)?,
                peso_gramos: validate_medida("peso_gramos", req.peso_gramos, 100_000)?,
                largo_cm: validate_medida("largo_cm", req.largo_cm, 500)?,
                ancho_cm: validate_medida("ancho_cm", req.ancho_cm, 500)?,
                alto_cm: validate_medida("alto_cm", req.alto_cm, 500)?,
                slug: match req.slug.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    Some(s) => {
                        let s = valid_slug("slug", s, 160)?;
                        if slug_taken(conn, &s, None)? { return Err(ApiError::conflict("Ya existe un producto con ese slug")); }
                        s
                    }
                    None => unique_slug(&slugify(&req.nombre), 160, |s| slug_taken(conn, s, None))?,
                },
            })
            .returning(Producto::as_returning())
            .get_result(conn)
            .map_err(map_product_err)?;
        for v in req.variantes {
            insert_variant(conn, producto.id, v)?;
        }
        for img in req.imagenes {
            insert_product_image(conn, producto.id, img)?;
        }
        detail(conn, producto.id, Some(&user))
    })
}

pub fn update(conn: &mut PgConnection, id: Uuid, req: UpdateProductoRequest, user: AuthUser) -> ApiResult<Producto> {
    get_owned_product(conn, id, &user)?;
    let UpdateProductoRequest { mut data, especificaciones, precio_anterior, garantia_meses } = req;
    if let Some(n) = &data.nombre {
        data.nombre = Some(required_text("nombre", n, 255)?);
    }
    if let Some(precio) = data.precio {
        non_negative("precio", precio)?;
    }
    if let Some(stock) = data.stock {
        non_negative("stock", stock)?;
    }
    for (field, max, value) in [("peso_gramos", 100_000, data.peso_gramos), ("largo_cm", 500, data.largo_cm),
                                ("ancho_cm", 500, data.ancho_cm), ("alto_cm", 500, data.alto_cm)] {
        validate_medida(field, value, max)?;
    }
    // Cadenas vacías => NULL
    let mut clear = Vec::new();
    for (field, value) in [("descripcion", &mut data.descripcion), ("sku", &mut data.sku),
                           ("marca", &mut data.marca), ("compatibilidad", &mut data.compatibilidad)] {
        if matches!(value, Some(v) if v.trim().is_empty()) {
            *value = None;
            clear.push(field);
        } else if let Some(v) = value {
            *v = v.trim().to_string();
        }
    }
    if let Some(m) = &data.marca {
        data.marca = Some(required_text("marca", m, 100)?);
    }
    if let Some(specs) = especificaciones {
        data.especificaciones = Some(validate_specs(specs)?);
    }
    data.updated_at = Some(now());
    let old_slug = productos::table.find(id).select(productos::slug).first::<String>(conn)?;
    if let Some(s) = data.slug.take() {
        let s = valid_slug("slug", &s, 160)?;
        if s != old_slug {
            if slug_taken(conn, &s, Some(id))? {
                return Err(ApiError::conflict("Ya existe un producto con ese slug"));
            }
            data.slug = Some(s);
        }
    }
    let slug_changed = data.slug.is_some();

    conn.transaction(|conn| {
        if slug_changed {
            // La URL anterior sigue funcionando (redirige a la nueva)
            diesel::delete(producto_slugs_antiguos::table.filter(producto_slugs_antiguos::slug.eq(data.slug.clone().unwrap_or_default()))).execute(conn)?;
            diesel::insert_into(producto_slugs_antiguos::table)
                .values((producto_slugs_antiguos::slug.eq(&old_slug), producto_slugs_antiguos::producto_id.eq(id)))
                .on_conflict_do_nothing()
                .execute(conn)?;
        }
        diesel::update(productos::table.find(id)).set(&data).execute(conn).map_err(map_product_err)?;
        for field in clear {
            let target = productos::table.find(id);
            match field {
                "descripcion" => diesel::update(target).set(productos::descripcion.eq(None::<String>)).execute(conn)?,
                "sku" => diesel::update(target).set(productos::sku.eq(None::<String>)).execute(conn)?,
                "marca" => diesel::update(target).set(productos::marca.eq(None::<String>)).execute(conn)?,
                _ => diesel::update(target).set(productos::compatibilidad.eq(None::<String>)).execute(conn)?,
            };
        }
        if let Some(pa) = precio_anterior {
            diesel::update(productos::table.find(id))
                .set(productos::precio_anterior.eq(pa))
                .execute(conn)
                .map_err(map_product_err)?;
        }
        if let Some(g) = garantia_meses {
            diesel::update(productos::table.find(id))
                .set(productos::garantia_meses.eq(validate_garantia(g)?))
                .execute(conn)?;
        }
        // Si cambió el precio, la regla precio_anterior > precio se comprueba en la BD
        Ok(productos::table.find(id).select(Producto::as_select()).first(conn)?)
    })
}

pub fn delete(conn: &mut PgConnection, id: Uuid, user: AuthUser) -> ApiResult<()> {
    get_owned_product(conn, id, &user)?;
    diesel::delete(productos::table.find(id)).execute(conn)?;
    Ok(())
}

// ---------- Variantes ----------

pub fn add_variant(conn: &mut PgConnection, producto_id: Uuid, req: NewVarianteRequest, user: AuthUser) -> ApiResult<Variante> {
    get_owned_product(conn, producto_id, &user)?;
    conn.transaction(|conn| insert_variant(conn, producto_id, req))
}

pub fn update_variant(conn: &mut PgConnection, id: Uuid, mut data: VarianteUpdate, user: AuthUser) -> ApiResult<Variante> {
    get_owned_variant(conn, id, &user)?;
    if let Some(s) = data.stock {
        non_negative("stock", s)?;
    }
    if let Some(pe) = data.precio_extra {
        non_negative("precio_extra", pe)?;
    }
    data.updated_at = Some(now());
    Ok(diesel::update(product_variants::table.find(id))
        .set(&data)
        .returning(Variante::as_returning())
        .get_result(conn)?)
}

pub fn delete_variant(conn: &mut PgConnection, id: Uuid, user: AuthUser) -> ApiResult<()> {
    get_owned_variant(conn, id, &user)?;
    diesel::delete(product_variants::table.find(id)).execute(conn)?;
    Ok(())
}

// ---------- Imágenes ----------

pub fn add_product_image(conn: &mut PgConnection, producto_id: Uuid, req: NewImageRequest, user: AuthUser) -> ApiResult<ProductImage> {
    get_owned_product(conn, producto_id, &user)?;
    conn.transaction(|conn| insert_product_image(conn, producto_id, req))
}

pub fn delete_product_image(conn: &mut PgConnection, image_id: Uuid, user: AuthUser) -> ApiResult<String> {
    let img: ProductImage = product_images::table
        .find(image_id)
        .select(ProductImage::as_select())
        .first(conn)
        .or_not_found("Imagen")?;
    get_owned_product(conn, img.product_id, &user)?;
    diesel::delete(product_images::table.find(image_id)).execute(conn)?;
    Ok(img.image_url)
}

pub fn add_variant_image(conn: &mut PgConnection, variant_id: Uuid, req: NewImageRequest, user: AuthUser) -> ApiResult<VariantImage> {
    get_owned_variant(conn, variant_id, &user)?;
    conn.transaction(|conn| insert_variant_image(conn, variant_id, req))
}

pub fn delete_variant_image(conn: &mut PgConnection, image_id: Uuid, user: AuthUser) -> ApiResult<String> {
    let img: VariantImage = product_variant_images::table
        .find(image_id)
        .select(VariantImage::as_select())
        .first(conn)
        .or_not_found("Imagen")?;
    get_owned_variant(conn, img.variant_id, &user)?;
    diesel::delete(product_variant_images::table.find(image_id)).execute(conn)?;
    Ok(img.image_url)
}


// ---------- Buscador: sugerencias (autocompletado) ----------

#[derive(Serialize)]
pub struct SugerenciaProducto {
    pub id: Uuid,
    pub slug: String,
    pub nombre: String,
    pub marca: Option<String>,
    pub precio: i64,
    pub precio_anterior: Option<i64>,
    pub imagen: Option<String>,
}

#[derive(Serialize)]
pub struct SugerenciaCategoria {
    pub slug: String,
    pub nombre: String,
}

#[derive(Serialize)]
pub struct Sugerencias {
    pub productos: Vec<SugerenciaProducto>,
    pub categorias: Vec<SugerenciaCategoria>,
    pub marcas: Vec<String>,
}

pub fn suggestions(conn: &mut PgConnection, q: &str) -> ApiResult<Sugerencias> {
    let q = q.trim();
    if q.chars().count() < 2 {
        return Ok(Sugerencias { productos: vec![], categorias: vec![], marcas: vec![] });
    }
    let q: String = q.chars().take(60).collect();
    let page = list(conn, ProductFilter { q: Some(q.clone()), orden: Some("relevancia".into()), ..Default::default() },
        Pagination { page: Some(1), per_page: Some(6) })?;
    let productos = page.items.into_iter().map(|i| SugerenciaProducto {
        id: i.producto.id, slug: i.producto.slug, nombre: i.producto.nombre, marca: i.producto.marca,
        precio: i.producto.precio, precio_anterior: i.producto.precio_anterior, imagen: i.imagen_principal,
    }).collect();
    let pattern = format!("%{q}%");
    let categorias = categorias::table
        .filter(categorias::estatus.eq(true))
        .filter(categorias::nombre.ilike(&pattern))
        .order(categorias::nombre)
        .limit(4)
        .select((categorias::slug, categorias::nombre))
        .load::<(String, String)>(conn)?
        .into_iter()
        .map(|(slug, nombre)| SugerenciaCategoria { slug, nombre })
        .collect();
    let marcas = productos::table
        .filter(productos::estatus.eq(true))
        .filter(productos::marca.ilike(&pattern))
        .select(productos::marca)
        .distinct()
        .limit(4)
        .load::<Option<String>>(conn)?
        .into_iter()
        .flatten()
        .collect();
    Ok(Sugerencias { productos, categorias, marcas })
}

// ---------- Complementarios ----------

#[derive(Serialize)]
pub struct Complementos {
    /// Elegidos por el vendedor
    pub sugeridos: Vec<ProductoListItem>,
    /// Calculados a partir de pedidos reales que incluyeron este producto
    pub comprados_juntos: Vec<ProductoListItem>,
}

pub fn complements(conn: &mut PgConnection, id: Uuid) -> ApiResult<Complementos> {
    let manual_ids: Vec<Uuid> = producto_complementos::table
        .filter(producto_complementos::producto_id.eq(id))
        .order(producto_complementos::orden)
        .select(producto_complementos::complemento_id)
        .load(conn)?;
    let mut by_id: HashMap<Uuid, Producto> = productos::table
        .filter(productos::id.eq_any(&manual_ids))
        .filter(productos::estatus.eq(true))
        .select(Producto::as_select())
        .load(conn)?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let sugeridos = to_list_items(conn, manual_ids.iter().filter_map(|i| by_id.remove(i)).collect())?;

    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        producto_id: Uuid,
    }
    let rows = diesel::sql_query(
        "SELECT d2.producto_id FROM detalle_orden d1 \
         JOIN ordenes o ON o.id = d1.orden_id AND o.estatus IN ('pagado', 'enviado', 'entregado') \
         JOIN detalle_orden d2 ON d2.orden_id = d1.orden_id AND d2.producto_id <> d1.producto_id \
         JOIN productos p ON p.id = d2.producto_id AND p.estatus \
         WHERE d1.producto_id = $1 AND d2.producto_id <> ALL($2) \
         GROUP BY d2.producto_id ORDER BY COUNT(DISTINCT d1.orden_id) DESC LIMIT 8",
    )
    .bind::<diesel::sql_types::Uuid, _>(id)
    .bind::<diesel::sql_types::Array<diesel::sql_types::Uuid>, _>(&manual_ids)
    .load::<Row>(conn)?;
    let ids: Vec<Uuid> = rows.into_iter().map(|r| r.producto_id).collect();
    let mut by_id: HashMap<Uuid, Producto> = productos::table
        .filter(productos::id.eq_any(&ids))
        .select(Producto::as_select())
        .load(conn)?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let comprados_juntos = to_list_items(conn, ids.iter().filter_map(|i| by_id.remove(i)).collect())?;
    Ok(Complementos { sugeridos, comprados_juntos })
}

#[derive(Deserialize)]
pub struct ComplementosRequest {
    pub ids: Vec<Uuid>,
}

pub fn set_complements(conn: &mut PgConnection, id: Uuid, ids: Vec<Uuid>, user: AuthUser) -> ApiResult<Complementos> {
    get_owned_product(conn, id, &user)?;
    if ids.len() > 12 {
        return Err(ApiError::bad_request("Máximo 12 productos complementarios"));
    }
    if ids.contains(&id) {
        return Err(ApiError::bad_request("Un producto no puede complementarse a sí mismo"));
    }
    conn.transaction(|conn| {
        diesel::delete(producto_complementos::table.filter(producto_complementos::producto_id.eq(id))).execute(conn)?;
        let mut seen = std::collections::HashSet::new();
        let rows: Vec<_> = ids.iter().filter(|c| seen.insert(**c)).enumerate().map(|(i, c)| (
            producto_complementos::producto_id.eq(id),
            producto_complementos::complemento_id.eq(*c),
            producto_complementos::orden.eq(i as i32),
        )).collect();
        diesel::insert_into(producto_complementos::table).values(&rows).execute(conn)?;
        complements(conn, id)
    })
}

#[derive(Serialize)]
pub struct MarcaInfo {
    pub marca: String,
    pub slug: String,
    pub total: i64,
}

/// Marca por su slug (para la página /marca/{slug}).
pub fn brand(conn: &mut PgConnection, slug: &str) -> ApiResult<MarcaInfo> {
    let rows: Vec<Option<String>> = productos::table
        .filter(productos::estatus.eq(true))
        .filter(sql::<diesel::sql_types::Bool>("jarstore_slugify(productos.marca) = ").bind::<Text, _>(slug.to_string()))
        .select(productos::marca)
        .load(conn)?;
    let marca = rows.iter().flatten().next().cloned().ok_or_else(|| ApiError::not_found("Marca no encontrada"))?;
    Ok(MarcaInfo { slug: slugify(&marca), total: rows.len() as i64, marca })
}
