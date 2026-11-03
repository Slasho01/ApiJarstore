use crate::errors::{ApiError, ApiResult, OrNotFound};
use crate::models::category::{Categoria, CategoriaUpdate, NewCategoria};
use crate::schema::categorias;
use crate::services::common::{now, optional_text, required_text, slugify, unique_slug, valid_slug};
use diesel::prelude::*;
use serde::Deserialize;
use uuid::Uuid;

/// Lista plana (el frontend arma el árbol con `parent_id`), ordenada por `orden` y nombre.
pub fn list(conn: &mut PgConnection, include_inactive: bool) -> ApiResult<Vec<Categoria>> {
    let mut q = categorias::table.into_boxed();
    if !include_inactive {
        q = q.filter(categorias::estatus.eq(true));
    }
    Ok(q.order((categorias::orden.asc(), categorias::nombre.asc())).select(Categoria::as_select()).load(conn)?)
}

/// Busca por id o por slug.
pub fn get(conn: &mut PgConnection, key: &str) -> ApiResult<Categoria> {
    let q = categorias::table.select(Categoria::as_select());
    match Uuid::parse_str(key) {
        Ok(id) => q.find(id).first(conn),
        Err(_) => q.filter(categorias::slug.eq(key)).first(conn),
    }
    .or_not_found("Categoría")
}

/// La categoría y todas sus descendientes (para filtrar productos de una categoría padre).
pub fn with_descendants(conn: &mut PgConnection, id: Uuid) -> ApiResult<Vec<Uuid>> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        id: Uuid,
    }
    let rows = diesel::sql_query(
        "WITH RECURSIVE t AS (SELECT id FROM categorias WHERE id = $1 \
         UNION SELECT c.id FROM categorias c JOIN t ON c.parent_id = t.id) SELECT id FROM t",
    )
    .bind::<diesel::sql_types::Uuid, _>(id)
    .load::<Row>(conn)?;
    Ok(rows.into_iter().map(|r| r.id).collect())
}

/// Ancestros desde la raíz hasta la categoría (para breadcrumbs).
pub fn ancestors(conn: &mut PgConnection, cat: &Categoria) -> ApiResult<Vec<Categoria>> {
    let mut chain = vec![];
    let mut parent = cat.parent_id;
    while let Some(pid) = parent {
        if chain.len() > 10 { break; }
        let p: Categoria = categorias::table.find(pid).select(Categoria::as_select()).first(conn)?;
        parent = p.parent_id;
        chain.push(p);
    }
    chain.reverse();
    Ok(chain)
}

fn slug_taken(conn: &mut PgConnection, slug: &str, except: Option<Uuid>) -> ApiResult<bool> {
    let mut q = categorias::table.filter(categorias::slug.eq(slug)).into_boxed();
    if let Some(id) = except {
        q = q.filter(categorias::id.ne(id));
    }
    Ok(diesel::select(diesel::dsl::exists(q)).get_result(conn)?)
}

fn check_parent(conn: &mut PgConnection, id: Option<Uuid>, parent: Option<Uuid>) -> ApiResult<()> {
    let Some(parent) = parent else { return Ok(()) };
    get(conn, &parent.to_string()).map_err(|_| ApiError::bad_request("La categoría padre no existe"))?;
    if let Some(id) = id {
        if with_descendants(conn, id)?.contains(&parent) {
            return Err(ApiError::bad_request("Una categoría no puede quedar dentro de sí misma o de sus subcategorías"));
        }
    }
    Ok(())
}

pub fn create(conn: &mut PgConnection, mut data: NewCategoria) -> ApiResult<Categoria> {
    data.nombre = required_text("nombre", &data.nombre, 255)?;
    data.descripcion = optional_text("descripcion", data.descripcion, 5000)?;
    data.icono = optional_text("icono", data.icono, 40)?;
    check_parent(conn, None, data.parent_id)?;
    data.slug = if data.slug.trim().is_empty() {
        unique_slug(&slugify(&data.nombre), 120, |s| slug_taken(conn, s, None))?
    } else {
        valid_slug("slug", &data.slug, 120)?
    };
    diesel::insert_into(categorias::table)
        .values(&data)
        .returning(Categoria::as_returning())
        .get_result(conn)
        .map_err(|e| match ApiError::from(e) {
            ApiError::Conflict(m) if m.contains("slug") => ApiError::conflict("Ya existe una categoría con ese slug"),
            ApiError::Conflict(_) => ApiError::conflict("Ya existe una categoría con ese nombre"),
            other => other,
        })
}

#[derive(Deserialize)]
pub struct CategoriaUpdateRequest {
    #[serde(flatten)]
    pub data: CategoriaUpdate,
    /// null => categoría raíz
    #[serde(default, deserialize_with = "crate::services::common::double_option")]
    pub parent_id: Option<Option<Uuid>>,
}

pub fn update(conn: &mut PgConnection, id: Uuid, req: CategoriaUpdateRequest) -> ApiResult<Categoria> {
    let CategoriaUpdateRequest { mut data, parent_id } = req;
    get(conn, &id.to_string())?;
    if let Some(n) = &data.nombre {
        data.nombre = Some(required_text("nombre", n, 255)?);
    }
    if let Some(s) = &data.slug {
        let s = valid_slug("slug", s, 120)?;
        if slug_taken(conn, &s, Some(id))? {
            return Err(ApiError::conflict("Ya existe una categoría con ese slug"));
        }
        data.slug = Some(s);
    }
    data.updated_at = Some(now());
    conn.transaction(|conn| {
        if let Some(parent) = parent_id {
            check_parent(conn, Some(id), parent)?;
            diesel::update(categorias::table.find(id)).set(categorias::parent_id.eq(parent)).execute(conn)?;
        }
        diesel::update(categorias::table.find(id))
            .set(&data)
            .returning(Categoria::as_returning())
            .get_result(conn)
            .map_err(|e| match ApiError::from(e) {
                ApiError::Conflict(_) => ApiError::conflict("Ya existe una categoría con ese nombre"),
                other => other,
            })
    })
}

/// Al borrar, sus subcategorías suben un nivel (no se pierden) y sus productos quedan sin categoría.
pub fn delete(conn: &mut PgConnection, id: Uuid) -> ApiResult<()> {
    conn.transaction(|conn| {
        let cat = get(conn, &id.to_string())?;
        diesel::update(categorias::table.filter(categorias::parent_id.eq(id)))
            .set(categorias::parent_id.eq(cat.parent_id))
            .execute(conn)?;
        diesel::delete(categorias::table.find(id)).execute(conn)?;
        Ok(())
    })
}
