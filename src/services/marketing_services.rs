//! Banners/promociones administrables y newsletter con doble confirmación.
use crate::errors::{ApiError, ApiResult, OrNotFound};
use crate::models::tienda::{Banner, BannerData, Suscriptor};
use crate::schema::{banners, newsletter};
use crate::services::common::{now, optional_text, required_text, valid_url, Paginated, Pagination};
use diesel::prelude::*;
use uuid::Uuid;

// ================= Banners =================

/// Banners vigentes (activos y dentro de su rango de fechas).
pub fn active_banners(conn: &mut PgConnection, ubicacion: Option<&str>) -> ApiResult<Vec<Banner>> {
    let n = now();
    let mut q = banners::table
        .filter(banners::activo.eq(true))
        .filter(banners::desde.is_null().or(banners::desde.le(n)))
        .filter(banners::hasta.is_null().or(banners::hasta.gt(n)))
        .into_boxed();
    if let Some(u) = ubicacion {
        q = q.filter(banners::ubicacion.eq(u.to_string()));
    }
    Ok(q.order((banners::orden.asc(), banners::created_at.desc())).select(Banner::as_select()).load(conn)?)
}

pub fn all_banners(conn: &mut PgConnection) -> ApiResult<Vec<Banner>> {
    Ok(banners::table.order((banners::ubicacion, banners::orden, banners::created_at.desc())).select(Banner::as_select()).load(conn)?)
}

fn clean_banner(mut b: BannerData) -> ApiResult<BannerData> {
    if !["hero", "promo"].contains(&b.ubicacion.as_str()) {
        return Err(ApiError::bad_request("ubicacion debe ser 'hero' o 'promo'"));
    }
    if !["claro", "oscuro"].contains(&b.tema.as_str()) {
        return Err(ApiError::bad_request("tema debe ser 'claro' u 'oscuro'"));
    }
    b.titulo = required_text("titulo", &b.titulo, 160)?;
    b.subtitulo = optional_text("subtitulo", b.subtitulo, 300)?;
    b.texto_boton = optional_text("texto_boton", b.texto_boton, 60)?;
    b.enlace = match optional_text("enlace", b.enlace, 500)? {
        Some(e) => Some(valid_url("enlace", &e)?),
        None => None,
    };
    for f in [&mut b.imagen_url, &mut b.imagen_movil_url] {
        *f = match f.take().filter(|s| !s.trim().is_empty()) { Some(u) => Some(valid_url("imagen", &u)?), None => None };
    }
    if !b.imagen_variantes.is_array() {
        return Err(ApiError::bad_request("imagen_variantes debe ser una lista"));
    }
    if let (Some(d), Some(h)) = (b.desde, b.hasta) {
        if h <= d {
            return Err(ApiError::bad_request("La fecha 'hasta' debe ser posterior a 'desde'"));
        }
    }
    b.updated_at = now();
    Ok(b)
}

pub fn create_banner(conn: &mut PgConnection, data: BannerData) -> ApiResult<Banner> {
    Ok(diesel::insert_into(banners::table).values(clean_banner(data)?).returning(Banner::as_returning()).get_result(conn)?)
}

pub fn update_banner(conn: &mut PgConnection, id: Uuid, data: BannerData) -> ApiResult<Banner> {
    diesel::update(banners::table.find(id)).set(clean_banner(data)?).returning(Banner::as_returning()).get_result(conn).or_not_found("Banner")
}

pub fn delete_banner(conn: &mut PgConnection, id: Uuid) -> ApiResult<Banner> {
    diesel::delete(banners::table.find(id)).returning(Banner::as_returning()).get_result(conn).or_not_found("Banner")
}

// ================= Newsletter =================

fn new_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

pub enum Alta {
    /// Hay que enviar el correo de confirmación con este token
    Confirmar(String),
    YaSuscrito,
}

pub fn subscribe(conn: &mut PgConnection, email: &str) -> ApiResult<Alta> {
    let email = required_text("email", email, 255)?.to_lowercase();
    let valid = email.split_once('@').is_some_and(|(l, d)| !l.is_empty() && d.contains('.')) && !email.contains(char::is_whitespace);
    if !valid {
        return Err(ApiError::bad_request("Email inválido"));
    }
    let existing: Option<Suscriptor> = newsletter::table.filter(newsletter::email.eq(&email)).select(Suscriptor::as_select()).first(conn).optional()?;
    match existing {
        Some(s) if s.confirmado_at.is_some() && s.baja_at.is_none() => Ok(Alta::YaSuscrito),
        Some(s) => {
            let token = new_token();
            diesel::update(newsletter::table.find(s.id))
                .set((newsletter::token.eq(&token), newsletter::baja_at.eq(None::<chrono::NaiveDateTime>), newsletter::confirmado_at.eq(None::<chrono::NaiveDateTime>)))
                .execute(conn)?;
            Ok(Alta::Confirmar(token))
        }
        None => {
            let token = new_token();
            diesel::insert_into(newsletter::table).values((newsletter::email.eq(&email), newsletter::token.eq(&token))).execute(conn)?;
            Ok(Alta::Confirmar(token))
        }
    }
}

pub fn confirm(conn: &mut PgConnection, token: &str) -> ApiResult<String> {
    diesel::update(newsletter::table.filter(newsletter::token.eq(token.trim())).filter(newsletter::baja_at.is_null()))
        .set(newsletter::confirmado_at.eq(now()))
        .returning(newsletter::email)
        .get_result(conn)
        .map_err(|_| ApiError::bad_request("El enlace de confirmación no es válido"))
}

pub fn unsubscribe(conn: &mut PgConnection, token: &str) -> ApiResult<String> {
    diesel::update(newsletter::table.filter(newsletter::token.eq(token.trim())))
        .set(newsletter::baja_at.eq(now()))
        .returning(newsletter::email)
        .get_result(conn)
        .map_err(|_| ApiError::bad_request("El enlace de baja no es válido"))
}

pub fn subscribers(conn: &mut PgConnection, p: Pagination, solo_confirmados: bool) -> ApiResult<Paginated<Suscriptor>> {
    let build = || {
        let mut q = newsletter::table.into_boxed();
        if solo_confirmados {
            q = q.filter(newsletter::confirmado_at.is_not_null()).filter(newsletter::baja_at.is_null());
        }
        q
    };
    let total: i64 = build().count().get_result(conn)?;
    let items = build().order(newsletter::created_at.desc()).limit(p.per_page()).offset(p.offset()).select(Suscriptor::as_select()).load(conn)?;
    Ok(Paginated::new(items, &p, total))
}

pub fn export_csv(conn: &mut PgConnection) -> ApiResult<String> {
    let rows: Vec<(String, Option<chrono::NaiveDateTime>)> = newsletter::table
        .filter(newsletter::confirmado_at.is_not_null())
        .filter(newsletter::baja_at.is_null())
        .order(newsletter::confirmado_at)
        .select((newsletter::email, newsletter::confirmado_at))
        .load(conn)?;
    let mut out = String::from("email,confirmado_utc\n");
    for (e, c) in rows {
        out.push_str(&format!("{},{}\n", e.replace([',', '\n', '"'], ""), c.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default()));
    }
    Ok(out)
}
