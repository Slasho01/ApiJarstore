use crate::auth::AuthUser;
use crate::db::{self, DbPool};
use crate::errors::ApiResult;
use crate::models::enum_role::Role;
use crate::models::products::VarianteUpdate;
use crate::services::common::Pagination;
use crate::controller::upload_controller::release;
use crate::services::product_services::{self as svc, BulkRequest, ComplementosRequest, FacetFilter, NewImageRequest, NewProductoRequest, NewVarianteRequest, ProductFilter, UpdateProductoRequest};
use crate::storage::Storage;
use serde::Deserialize;
use actix_web::{web, HttpResponse};
use uuid::Uuid;

const SELLERS: &[Role] = &[Role::Admin, Role::Seller];

/// Catálogo público con filtros y paginación.
pub async fn list(
    filter: web::Query<ProductFilter>,
    page: web::Query<Pagination>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    let (f, p) = (filter.into_inner(), page.into_inner());
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::list(conn, f, p)).await?))
}

/// Productos del vendedor autenticado, incluidos los inactivos.
pub async fn mine(
    user: AuthUser,
    filter: web::Query<ProductFilter>,
    page: web::Query<Pagination>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    let mut f = filter.into_inner();
    f.vendedor_id = Some(user.id);
    f.incluir_inactivos = true;
    let p = page.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::list(conn, f, p)).await?))
}

#[derive(Deserialize)]
pub struct LimitQuery {
    pub limit: Option<i64>,
}

pub async fn facets(filter: web::Query<FacetFilter>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let f = filter.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::facets(conn, f)).await?))
}

pub async fn best_sellers(q: web::Query<LimitQuery>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let limit = q.limit.unwrap_or(8);
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::best_sellers(conn, limit)).await?))
}

pub async fn related(path: web::Path<Uuid>, q: web::Query<LimitQuery>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let (id, limit) = (path.into_inner(), q.limit.unwrap_or(8));
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::related(conn, id, limit)).await?))
}

/// Detalle por id, slug o slug antiguo. El campo `slug` de la respuesta es la URL canónica.
pub async fn detail(path: web::Path<String>, user: Option<AuthUser>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let key = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| {
        let id = svc::resolve_key(conn, &key)?;
        svc::detail(conn, id, user.as_ref())
    }).await?))
}

#[derive(Deserialize)]
pub struct SuggestQuery {
    pub q: String,
}

pub async fn suggestions(q: web::Query<SuggestQuery>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let q = q.into_inner().q;
    Ok(HttpResponse::Ok()
        .insert_header(("Cache-Control", "public, max-age=30"))
        .json(db::run(&pool, move |conn| svc::suggestions(conn, &q)).await?))
}

pub async fn brand(path: web::Path<String>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let slug = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::brand(conn, &slug)).await?))
}

pub async fn complements(path: web::Path<Uuid>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::complements(conn, id)).await?))
}

pub async fn set_complements(path: web::Path<Uuid>, user: AuthUser, body: web::Json<ComplementosRequest>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    let id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::set_complements(conn, id, body.into_inner().ids, user)).await?))
}

pub async fn create(user: AuthUser, body: web::Json<NewProductoRequest>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    Ok(HttpResponse::Created().json(db::run(&pool, move |conn| svc::create(conn, body.into_inner(), user)).await?))
}

pub async fn update(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<UpdateProductoRequest>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    let id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::update(conn, id, body.into_inner(), user)).await?))
}

pub async fn delete(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    let id = path.into_inner();
    db::run(&pool, move |conn| svc::delete(conn, id, user)).await?;
    Ok(HttpResponse::NoContent().finish())
}

/// Acción masiva: pausar, reactivar o eliminar varios productos seleccionados.
pub async fn bulk(user: AuthUser, body: web::Json<BulkRequest>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    let afectados = db::run(&pool, move |conn| svc::bulk(conn, body.into_inner(), user)).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "afectados": afectados })))
}

// ---------- Variantes ----------

pub async fn add_variant(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<NewVarianteRequest>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    let id = path.into_inner();
    Ok(HttpResponse::Created().json(db::run(&pool, move |conn| svc::add_variant(conn, id, body.into_inner(), user)).await?))
}

pub async fn update_variant(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<VarianteUpdate>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    let id = path.into_inner();
    Ok(HttpResponse::Ok().json(db::run(&pool, move |conn| svc::update_variant(conn, id, body.into_inner(), user)).await?))
}

pub async fn delete_variant(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    let id = path.into_inner();
    db::run(&pool, move |conn| svc::delete_variant(conn, id, user)).await?;
    Ok(HttpResponse::NoContent().finish())
}

// ---------- Imágenes ----------

pub async fn add_product_image(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<NewImageRequest>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    let id = path.into_inner();
    Ok(HttpResponse::Created().json(db::run(&pool, move |conn| svc::add_product_image(conn, id, body.into_inner(), user)).await?))
}

pub async fn delete_product_image(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>, storage: web::Data<Storage>) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    let id = path.into_inner();
    let url = db::run(&pool, move |conn| svc::delete_product_image(conn, id, user)).await?;
    release(&pool, &storage, url).await;
    Ok(HttpResponse::NoContent().finish())
}

pub async fn add_variant_image(
    path: web::Path<Uuid>,
    user: AuthUser,
    body: web::Json<NewImageRequest>,
    pool: web::Data<DbPool>,
) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    let id = path.into_inner();
    Ok(HttpResponse::Created().json(db::run(&pool, move |conn| svc::add_variant_image(conn, id, body.into_inner(), user)).await?))
}

pub async fn delete_variant_image(path: web::Path<Uuid>, user: AuthUser, pool: web::Data<DbPool>, storage: web::Data<Storage>) -> ApiResult<HttpResponse> {
    user.require(SELLERS)?;
    let id = path.into_inner();
    let url = db::run(&pool, move |conn| svc::delete_variant_image(conn, id, user)).await?;
    release(&pool, &storage, url).await;
    Ok(HttpResponse::NoContent().finish())
}
