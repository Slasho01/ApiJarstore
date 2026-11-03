//! Configuración editable de la tienda (tienda, monedas, envío, pagos) y páginas de contenido.
use crate::errors::{ApiError, ApiResult, OrNotFound};
use crate::models::tienda::*;
use crate::schema::{configuracion, paginas};
use crate::services::common::{now, required_text};
use diesel::prelude::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashSet;
use uuid::Uuid;

fn load<T: DeserializeOwned>(conn: &mut PgConnection, clave: &str) -> ApiResult<T> {
    let valor: serde_json::Value = configuracion::table
        .find(clave)
        .select(configuracion::valor)
        .first(conn)
        .or_not_found("Configuración")?;
    serde_json::from_value(valor).map_err(|e| ApiError::internal(format!("Configuración '{clave}' corrupta: {e}")))
}

pub fn monedas(conn: &mut PgConnection) -> ApiResult<MonedasConfig> { load(conn, "monedas") }
pub fn envio(conn: &mut PgConnection) -> ApiResult<EnvioConfig> { load(conn, "envio") }
pub fn pagos(conn: &mut PgConnection) -> ApiResult<PagosConfig> { load(conn, "pagos") }
pub fn tienda(conn: &mut PgConnection) -> ApiResult<TiendaConfig> { load(conn, "tienda") }

/// Configuración pública que necesita el frontend en cada carga.
#[derive(Serialize)]
pub struct PublicConfig {
    pub tienda: TiendaConfig,
    pub puntos: PuntosConfig,
    /// Sólo monedas utilizables (con tasa configurada)
    pub monedas: MonedasConfig,
    pub envio: EnvioConfig,
    pub pagos: PagosConfig,
    pub paginas: Vec<PaginaLink>,
}

pub fn public_config(conn: &mut PgConnection, mp_enabled: bool) -> ApiResult<PublicConfig> {
    let mut monedas = monedas(conn)?;
    let base = monedas.base.clone();
    monedas.lista.retain(|m| m.codigo == base || m.tasa.is_some_and(|t| t > 0.0));
    let paginas = paginas::table
        .filter(paginas::visible.eq(true))
        .order((paginas::orden.asc(), paginas::titulo.asc()))
        .select(PaginaLink::as_select())
        .load(conn)?;
    let mut pagos = pagos(conn)?;
    // Mercado Pago sólo se ofrece si el servidor tiene credenciales
    pagos.metodos.retain(|m| m.tipo != "mercadopago" || mp_enabled);
    for m in &mut pagos.metodos {
        if m.tipo == "mercadopago" {
            m.instrucciones = None;
        }
    }
    Ok(PublicConfig { tienda: tienda(conn)?, puntos: crate::services::puntos_services::config(conn)?, monedas, envio: envio(conn)?, pagos, paginas })
}

/// Configuración completa para el panel (incluye monedas sin tasa).
pub fn admin_config(conn: &mut PgConnection) -> ApiResult<Vec<ConfigEntry>> {
    let mut entries: Vec<ConfigEntry> = configuracion::table.order(configuracion::clave).select(ConfigEntry::as_select()).load(conn)?;
    // No exponer la contraseña SMTP; sólo indicar si está guardada
    for e in entries.iter_mut() {
        if e.clave == "smtp" {
            let set = e.valor.get("password").and_then(|p| p.as_str()).map(|p| !p.is_empty()).unwrap_or(false);
            if let Some(obj) = e.valor.as_object_mut() {
                obj.insert("password".into(), serde_json::json!(""));
                obj.insert("password_set".into(), serde_json::json!(set));
            }
        }
    }
    Ok(entries)
}

fn parse<T: DeserializeOwned>(clave: &str, valor: serde_json::Value) -> ApiResult<T> {
    serde_json::from_value(valor).map_err(|e| ApiError::bad_request(format!("Formato inválido para '{clave}': {e}")))
}

fn valid_id(field: &str, id: &str) -> ApiResult<()> {
    if id.is_empty() || id.len() > 50 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(ApiError::bad_request(format!("'{field}' sólo admite letras, números, '-' y '_' (máx. 50)")));
    }
    Ok(())
}

fn unique<'a>(what: &str, ids: impl Iterator<Item = &'a str>) -> ApiResult<()> {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(ApiError::bad_request(format!("{what} repetido: '{id}'")));
        }
    }
    Ok(())
}

/// Valida y normaliza cada tipo de configuración antes de guardarla.
fn validate(clave: &str, valor: serde_json::Value) -> ApiResult<serde_json::Value> {
    let out = match clave {
        "tienda" => {
            let mut t: TiendaConfig = parse(clave, valor)?;
            t.nombre = required_text("nombre", &t.nombre, 100)?;
            for b in &mut t.beneficios {
                b.titulo = required_text("titulo del beneficio", &b.titulo, 100)?;
            }
            serde_json::to_value(t)
        }
        "monedas" => {
            let mut m: MonedasConfig = parse(clave, valor)?;
            for x in &mut m.lista {
                x.codigo = x.codigo.trim().to_uppercase();
                if x.codigo.len() != 3 || !x.codigo.chars().all(|c| c.is_ascii_alphabetic()) {
                    return Err(ApiError::bad_request(format!("Código de moneda inválido: '{}' (usa ISO 4217, p. ej. CLP)", x.codigo)));
                }
                if x.decimales > 4 {
                    return Err(ApiError::bad_request("Una moneda admite máximo 4 decimales"));
                }
                if matches!(x.tasa, Some(t) if !(t > 0.0 && t.is_finite())) {
                    return Err(ApiError::bad_request(format!("La tasa de {} debe ser mayor que 0", x.codigo)));
                }
            }
            unique("Código de moneda", m.lista.iter().map(|x| x.codigo.as_str()))?;
            m.base = m.base.trim().to_uppercase();
            let base = m.lista.iter_mut().find(|x| x.codigo == m.base)
                .ok_or_else(|| ApiError::bad_request("La moneda base debe estar en la lista"))?;
            base.tasa = Some(1.0);
            serde_json::to_value(m)
        }
        "envio" => {
            let mut e: EnvioConfig = parse(clave, valor)?;
            for p in &mut e.paises {
                p.codigo = p.codigo.trim().to_uppercase();
                if p.codigo.len() != 2 {
                    return Err(ApiError::bad_request(format!("Código de país inválido: '{}' (usa ISO, p. ej. CL)", p.codigo)));
                }
                p.nombre = required_text("nombre del país", &p.nombre, 100)?;
                for m in &mut p.metodos {
                    valid_id("id del método de envío", &m.id)?;
                    m.nombre = required_text("nombre del método de envío", &m.nombre, 100)?;
                    if m.precio < 0 || matches!(m.gratis_desde, Some(g) if g < 0) {
                        return Err(ApiError::bad_request("Los montos de envío no pueden ser negativos"));
                    }
                }
                unique("Método de envío", p.metodos.iter().map(|m| m.id.as_str()))?;
                if let Some(c) = &mut p.courier {
                    c.origen = c.origen.trim().to_uppercase();
                    if c.recargo < 0 || matches!(c.gratis_desde, Some(g) if g < 0) {
                        return Err(ApiError::bad_request("El recargo y el umbral de envío gratis no pueden ser negativos"));
                    }
                    if p.modo == crate::models::tienda::ModoEnvio::Courier && c.origen.is_empty() {
                        return Err(ApiError::bad_request(format!("Indica la comuna de origen del courier para {}", p.nombre)));
                    }
                }
                if let Some(pe) = &mut p.peso {
                    if pe.base < 0 || pe.precio_por_kg < 0 || pe.factor_volumetrico <= 0 || matches!(pe.gratis_desde, Some(g) if g < 0) {
                        return Err(ApiError::bad_request("Los montos del envío por peso no pueden ser negativos y el factor volumétrico debe ser mayor que 0"));
                    }
                    pe.nombre = pe.nombre.as_ref().map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
                    if matches!(&pe.nombre, Some(n) if n.chars().count() > 100) {
                        return Err(ApiError::bad_request("El nombre del envío por peso admite máximo 100 caracteres"));
                    }
                }
            }
            unique("País", e.paises.iter().map(|p| p.codigo.as_str()))?;
            serde_json::to_value(e)
        }
        "pagos" => {
            let mut p: PagosConfig = parse(clave, valor)?;
            for m in &mut p.metodos {
                valid_id("id del método de pago", &m.id)?;
                m.nombre = required_text("nombre del método de pago", &m.nombre, 100)?;
                if !["manual", "mercadopago"].contains(&m.tipo.as_str()) {
                    return Err(ApiError::bad_request("El tipo de pago debe ser 'manual' o 'mercadopago'"));
                }
                m.paises = m.paises.iter().map(|c| c.trim().to_uppercase()).filter(|c| c.len() == 2).collect();
            }
            if p.metodos.iter().filter(|m| m.tipo == "mercadopago").count() > 1 {
                return Err(ApiError::bad_request("Sólo puede haber un medio de pago de Mercado Pago"));
            }
            unique("Método de pago", p.metodos.iter().map(|m| m.id.as_str()))?;
            serde_json::to_value(p)
        }
        "smtp" => {
            let mut c: SmtpConfig = parse(clave, valor)?;
            if !["starttls", "tls", "none"].contains(&c.security.as_str()) {
                return Err(ApiError::bad_request("La seguridad SMTP debe ser 'starttls', 'tls' o 'none'"));
            }
            if c.port == 0 {
                return Err(ApiError::bad_request("Puerto SMTP inválido"));
            }
            c.host = c.host.trim().to_string();
            c.usuario = c.usuario.trim().to_string();
            c.from_email = c.from_email.trim().to_string();
            c.from_nombre = c.from_nombre.trim().to_string();
            // Si se marca activo, exigir host y remitente válidos
            if c.activo {
                if c.host.is_empty() {
                    return Err(ApiError::bad_request("Indica el servidor SMTP (host)"));
                }
                if c.from_email.is_empty() || !c.from_email.contains('@') {
                    return Err(ApiError::bad_request("Indica un correo remitente (from) válido"));
                }
            }
            serde_json::to_value(c)
        }
        "puntos" => {
            let mut p: PuntosConfig = parse(clave, valor)?;
            p.nombre = required_text("nombre del programa", &p.nombre, 60)?;
            if p.monto_base <= 0 || p.puntos_por_monto < 0 || p.valor_punto < 0 || p.minimo_canje < 0 {
                return Err(ApiError::bad_request("Los valores del programa de puntos deben ser positivos"));
            }
            if !(0..=100).contains(&p.maximo_porcentaje) {
                return Err(ApiError::bad_request("El máximo canjeable debe estar entre 0% y 100%"));
            }
            // Evita reglas que regalen más de lo que se gasta (p. ej. 1 punto = $50 por cada $1.000)
            if p.puntos_por_monto * p.valor_punto > p.monto_base {
                return Err(ApiError::bad_request("Con esas reglas el cliente recuperaría más del 100% de lo gastado"));
            }
            serde_json::to_value(p)
        }
        other => return Err(ApiError::not_found(format!("Configuración '{other}' no existe"))),
    };
    out.map_err(|e| ApiError::internal(e.to_string()))
}

pub fn set_config(conn: &mut PgConnection, clave: &str, mut valor: serde_json::Value) -> ApiResult<ConfigEntry> {
    // SMTP: si no se envía contraseña nueva, conservar la guardada (no se expone al editar)
    if clave == "smtp" && valor.get("password").and_then(|p| p.as_str()).unwrap_or("").is_empty() {
        if let Ok(actual) = configuracion::table.find("smtp").select(configuracion::valor).first::<serde_json::Value>(conn) {
            if let Some(pw) = actual.get("password").cloned() {
                valor["password"] = pw;
            }
        }
    }
    let valor = validate(clave, valor)?;
    Ok(diesel::insert_into(configuracion::table)
        .values((configuracion::clave.eq(clave), configuracion::valor.eq(&valor)))
        .on_conflict(configuracion::clave)
        .do_update()
        .set((configuracion::valor.eq(&valor), configuracion::updated_at.eq(now())))
        .returning(ConfigEntry::as_returning())
        .get_result(conn)?)
}

// ================= Páginas =================

fn valid_slug(slug: &str) -> ApiResult<String> {
    let s = slug.trim().to_lowercase();
    let ok = !s.is_empty() && s.len() <= 100
        && s.split('-').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    if !ok {
        return Err(ApiError::bad_request("El slug sólo admite minúsculas, números y guiones (p. ej. 'cambios-y-devoluciones')"));
    }
    Ok(s)
}

pub fn page_by_slug(conn: &mut PgConnection, slug: &str, include_hidden: bool) -> ApiResult<Pagina> {
    let mut q = paginas::table.filter(paginas::slug.eq(slug)).into_boxed();
    if !include_hidden {
        q = q.filter(paginas::visible.eq(true));
    }
    q.select(Pagina::as_select()).first(conn).or_not_found("Página")
}

pub fn pages_all(conn: &mut PgConnection) -> ApiResult<Vec<Pagina>> {
    Ok(paginas::table.order((paginas::orden.asc(), paginas::titulo.asc())).select(Pagina::as_select()).load(conn)?)
}

fn conflict_slug(e: diesel::result::Error) -> ApiError {
    match ApiError::from(e) {
        ApiError::Conflict(_) => ApiError::conflict("Ya existe una página con ese slug"),
        other => other,
    }
}

pub fn page_create(conn: &mut PgConnection, mut data: NewPagina) -> ApiResult<Pagina> {
    data.slug = valid_slug(&data.slug)?;
    data.titulo = required_text("titulo", &data.titulo, 255)?;
    diesel::insert_into(paginas::table)
        .values(&data)
        .returning(Pagina::as_returning())
        .get_result(conn)
        .map_err(conflict_slug)
}

pub fn page_update(conn: &mut PgConnection, id: Uuid, mut data: PaginaUpdate) -> ApiResult<Pagina> {
    if let Some(s) = &data.slug {
        data.slug = Some(valid_slug(s)?);
    }
    if let Some(t) = &data.titulo {
        data.titulo = Some(required_text("titulo", t, 255)?);
    }
    data.updated_at = Some(now());
    diesel::update(paginas::table.find(id))
        .set(&data)
        .returning(Pagina::as_returning())
        .get_result(conn)
        .map_err(|e| match e {
            diesel::result::Error::NotFound => ApiError::not_found("Página no encontrada"),
            e => conflict_slug(e),
        })
}

pub fn page_delete(conn: &mut PgConnection, id: Uuid) -> ApiResult<()> {
    match diesel::delete(paginas::table.find(id)).execute(conn)? {
        0 => Err(ApiError::not_found("Página no encontrada")),
        _ => Ok(()),
    }
}

/// Formatea un monto en moneda base convertido a `moneda` (p. ej. "$12.990" o "Bs. 1.234,50").
pub fn format_money(amount_base: i64, moneda: &Moneda, tasa: f64, base_decimales: u32) -> String {
    let value = amount_base as f64 / 10f64.powi(base_decimales as i32) * tasa;
    let factor = 10f64.powi(moneda.decimales as i32);
    let units = (value * factor).round() as i64;
    let (int, frac) = (units.abs() / factor as i64, units.abs() % factor as i64);
    let digits = int.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(c);
    }
    let sign = if units < 0 { "-" } else { "" };
    if moneda.decimales > 0 {
        format!("{sign}{} {grouped},{:0width$}", moneda.simbolo, frac, width = moneda.decimales as usize)
    } else {
        format!("{sign}{}{grouped}", moneda.simbolo)
    }
}
