//! Configuración de la tienda, páginas de contenido y recuperación de contraseña.
use crate::schema::{configuracion, paginas, password_resets};
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = configuracion, check_for_backend(diesel::pg::Pg))]
pub struct ConfigEntry {
    pub clave: String,
    pub valor: serde_json::Value,
    pub updated_at: NaiveDateTime,
}

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = paginas, check_for_backend(diesel::pg::Pg))]
pub struct Pagina {
    pub id: Uuid,
    pub slug: String,
    pub titulo: String,
    pub contenido: String,
    pub visible: bool,
    pub en_footer: bool,
    pub orden: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Versión resumida para menús y footer.
#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = paginas, check_for_backend(diesel::pg::Pg))]
pub struct PaginaLink {
    pub slug: String,
    pub titulo: String,
    pub en_footer: bool,
}

#[derive(Insertable, Deserialize, Debug)]
#[diesel(table_name = paginas)]
pub struct NewPagina {
    pub slug: String,
    pub titulo: String,
    #[serde(default)]
    pub contenido: String,
    #[serde(default)]
    pub visible: bool,
    #[serde(default = "crate::models::category::default_true")]
    pub en_footer: bool,
    #[serde(default)]
    pub orden: i32,
}

#[derive(AsChangeset, Deserialize, Debug)]
#[diesel(table_name = paginas)]
pub struct PaginaUpdate {
    pub slug: Option<String>,
    pub titulo: Option<String>,
    pub contenido: Option<String>,
    pub visible: Option<bool>,
    pub en_footer: Option<bool>,
    pub orden: Option<i32>,
    #[serde(skip)]
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = password_resets)]
pub struct NewPasswordReset {
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: NaiveDateTime,
}

// ---------- Estructura tipada de los valores de configuración ----------

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Moneda {
    pub codigo: String,
    pub nombre: String,
    pub simbolo: String,
    pub decimales: u32,
    /// Unidades de esta moneda por 1 unidad de la moneda base. `None` = sin configurar (no se ofrece).
    pub tasa: Option<f64>,
    #[serde(default)]
    pub locale: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MonedasConfig {
    pub base: String,
    pub lista: Vec<Moneda>,
}

impl MonedasConfig {
    /// Moneda activa (con tasa) por código. La base siempre tiene tasa 1.
    pub fn find(&self, codigo: &str) -> Option<(&Moneda, f64)> {
        self.lista.iter().find(|m| m.codigo == codigo).and_then(|m| {
            if m.codigo == self.base { Some((m, 1.0)) } else { m.tasa.filter(|t| *t > 0.0).map(|t| (m, t)) }
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MetodoEnvio {
    pub id: String,
    pub nombre: String,
    /// Costo en moneda base
    pub precio: i64,
    /// Si el subtotal (con descuento) alcanza este monto, el envío es gratis
    #[serde(default)]
    pub gratis_desde: Option<i64>,
    #[serde(default)]
    pub plazo: Option<String>,
    #[serde(default)]
    pub descripcion: Option<String>,
}

impl MetodoEnvio {
    pub fn costo_para(&self, subtotal_con_descuento: i64) -> i64 {
        match self.gratis_desde {
            Some(limite) if subtotal_con_descuento >= limite => 0,
            _ => self.precio,
        }
    }
}

/// Cómo se calcula el envío para un país.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModoEnvio {
    /// Tarifas planas configuradas a mano (por defecto).
    #[default]
    Manual,
    /// Cotización en tiempo real con un courier (Blue Express, Chile).
    Courier,
    /// Cálculo por peso/tamaño del pedido (Venezuela).
    Peso,
}

/// Parámetros cuando el país usa courier (Blue Express).
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CourierEnvio {
    /// Comuna de origen (código del courier) desde donde despachas.
    #[serde(default)]
    pub origen: String,
    /// Margen que la tienda suma a la tarifa del courier (moneda base).
    #[serde(default)]
    pub recargo: i64,
    /// Envío gratis si el subtotal (con descuento) alcanza este monto.
    #[serde(default)]
    pub gratis_desde: Option<i64>,
}

/// Parámetros cuando el país cobra por peso/tamaño (peso volumétrico).
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct PesoEnvio {
    #[serde(default)]
    pub nombre: Option<String>,
    /// Cargo fijo base (moneda base).
    #[serde(default)]
    pub base: i64,
    /// Precio por cada kg facturable (moneda base).
    #[serde(default)]
    pub precio_por_kg: i64,
    /// cm³ por kg para el peso volumétrico (Blue Express usa 4000).
    #[serde(default = "factor_volumetrico_default")]
    pub factor_volumetrico: i64,
    #[serde(default)]
    pub gratis_desde: Option<i64>,
    #[serde(default)]
    pub plazo: Option<String>,
}

fn factor_volumetrico_default() -> i64 {
    4000
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PaisEnvio {
    pub codigo: String,
    pub nombre: String,
    #[serde(default)]
    pub modo: ModoEnvio,
    #[serde(default)]
    pub metodos: Vec<MetodoEnvio>,
    #[serde(default)]
    pub courier: Option<CourierEnvio>,
    #[serde(default)]
    pub peso: Option<PesoEnvio>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnvioConfig {
    pub paises: Vec<PaisEnvio>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MetodoPago {
    pub id: String,
    pub nombre: String,
    /// "manual" (transferencia, efectivo, Pago Móvil…) o "mercadopago"
    #[serde(default = "manual")]
    pub tipo: String,
    /// Países donde se ofrece (vacío = todos)
    #[serde(default)]
    pub paises: Vec<String>,
    /// Instrucciones que ve el cliente al confirmar (p. ej. datos de transferencia)
    #[serde(default)]
    pub instrucciones: Option<String>,
}

fn manual() -> String { "manual".into() }

impl MetodoPago {
    pub fn disponible_en(&self, pais: &str) -> bool {
        self.paises.is_empty() || self.paises.iter().any(|p| p.eq_ignore_ascii_case(pais))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PagosConfig {
    pub metodos: Vec<MetodoPago>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Beneficio {
    /// Nombre del icono en el frontend: truck, shield, refresh, lock, card, support, store, clock
    pub icono: String,
    pub titulo: String,
    #[serde(default)]
    pub texto: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Redes {
    #[serde(default)]
    pub instagram: String,
    #[serde(default)]
    pub facebook: String,
    #[serde(default)]
    pub tiktok: String,
    #[serde(default)]
    pub x: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TiendaConfig {
    pub nombre: String,
    #[serde(default)]
    pub descripcion: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub telefono: String,
    #[serde(default)]
    pub whatsapp: String,
    #[serde(default)]
    pub direccion: String,
    #[serde(default)]
    pub redes: Redes,
    #[serde(default)]
    pub beneficios: Vec<Beneficio>,
}

// ---------- Subidas, banners y newsletter ----------
use crate::schema::{banners, newsletter, uploads};

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = uploads, check_for_backend(diesel::pg::Pg))]
pub struct Upload {
    pub id: Uuid,
    pub proveedor: String,
    pub clave: String,
    pub url: String,
    pub variantes: serde_json::Value,
    pub ancho: Option<i32>,
    pub alto: Option<i32>,
    pub bytes: Option<i64>,
    pub created_at: NaiveDateTime,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = uploads)]
pub struct NewUpload {
    pub user_id: Option<Uuid>,
    pub proveedor: String,
    pub clave: String,
    pub url: String,
    pub variantes: serde_json::Value,
    pub ancho: Option<i32>,
    pub alto: Option<i32>,
    pub bytes: Option<i64>,
}

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = banners, check_for_backend(diesel::pg::Pg))]
pub struct Banner {
    pub id: Uuid,
    pub ubicacion: String,
    pub titulo: String,
    pub subtitulo: Option<String>,
    pub texto_boton: Option<String>,
    pub enlace: Option<String>,
    pub imagen_url: Option<String>,
    pub imagen_variantes: serde_json::Value,
    pub imagen_movil_url: Option<String>,
    pub tema: String,
    pub orden: i32,
    pub activo: bool,
    pub desde: Option<NaiveDateTime>,
    pub hasta: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// Crear y reemplazar (PUT) un banner.
#[derive(Insertable, AsChangeset, Deserialize, Debug)]
#[diesel(table_name = banners, treat_none_as_null = true)]
pub struct BannerData {
    #[serde(default = "hero")]
    pub ubicacion: String,
    pub titulo: String,
    pub subtitulo: Option<String>,
    pub texto_boton: Option<String>,
    pub enlace: Option<String>,
    pub imagen_url: Option<String>,
    #[serde(default = "empty_array")]
    pub imagen_variantes: serde_json::Value,
    pub imagen_movil_url: Option<String>,
    #[serde(default = "claro")]
    pub tema: String,
    #[serde(default)]
    pub orden: i32,
    #[serde(default = "crate::models::category::default_true")]
    pub activo: bool,
    pub desde: Option<NaiveDateTime>,
    pub hasta: Option<NaiveDateTime>,
    #[serde(skip, default = "crate::services::common::now")]
    pub updated_at: NaiveDateTime,
}
fn hero() -> String { "hero".into() }
fn claro() -> String { "claro".into() }
fn empty_array() -> serde_json::Value { serde_json::json!([]) }

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = newsletter, check_for_backend(diesel::pg::Pg))]
pub struct Suscriptor {
    pub id: Uuid,
    pub email: String,
    pub confirmado_at: Option<NaiveDateTime>,
    pub baja_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

// ---------- Programa de puntos ----------
use crate::schema::puntos_movimientos;

/// Reglas: se ganan `puntos_por_monto` puntos por cada `monto_base` gastado;
/// cada punto vale `valor_punto` al canjearlo (todo en moneda base).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PuntosConfig {
    pub activo: bool,
    pub nombre: String,
    pub monto_base: i64,
    pub puntos_por_monto: i64,
    pub valor_punto: i64,
    /// Puntos mínimos para poder canjear
    pub minimo_canje: i64,
    /// Máximo % del pedido (productos menos cupón) que se puede pagar con puntos
    pub maximo_porcentaje: i64,
}

impl PuntosConfig {
    /// Puntos que genera un gasto (en moneda base).
    pub fn ganados_por(&self, gasto: i64) -> i64 {
        if !self.activo || self.monto_base <= 0 || gasto <= 0 { return 0; }
        (gasto / self.monto_base) * self.puntos_por_monto
    }
    /// Máximo de puntos usables en un pedido con ese monto canjeable.
    pub fn max_usables(&self, saldo: i64, monto: i64) -> i64 {
        if !self.activo || self.valor_punto <= 0 || saldo < self.minimo_canje { return 0; }
        let tope = monto * self.maximo_porcentaje.clamp(0, 100) / 100 / self.valor_punto;
        saldo.min(tope).max(0)
    }
}

#[derive(Queryable, Selectable, Serialize, Debug)]
#[diesel(table_name = puntos_movimientos, check_for_backend(diesel::pg::Pg))]
pub struct MovimientoPuntos {
    pub id: Uuid,
    pub orden_id: Option<Uuid>,
    pub tipo: String,
    pub puntos: i32,
    pub descripcion: Option<String>,
    pub created_at: NaiveDateTime,
}

#[cfg(test)]
mod tests_puntos {
    use super::PuntosConfig;
    fn cfg() -> PuntosConfig {
        PuntosConfig { activo: true, nombre: "P".into(), monto_base: 1000, puntos_por_monto: 1, valor_punto: 10, minimo_canje: 100, maximo_porcentaje: 50 }
    }
    #[test]
    fn reglas() {
        let c = cfg();
        assert_eq!(c.ganados_por(12_990), 12);
        assert_eq!(c.ganados_por(999), 0);
        assert_eq!(c.max_usables(99, 100_000), 0); // bajo el mínimo
        assert_eq!(c.max_usables(500, 100_000), 500); // tope 50% = 5.000 puntos
        assert_eq!(c.max_usables(9_000, 100_000), 5_000);
        assert_eq!(PuntosConfig { activo: false, ..cfg() }.ganados_por(50_000), 0);
    }
}

// ---------- Configuración SMTP (correo) ----------
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SmtpConfig {
    #[serde(default)]
    pub activo: bool,
    #[serde(default)]
    pub host: String,
    #[serde(default = "smtp_port")]
    pub port: u16,
    /// starttls (587) | tls (465) | none
    #[serde(default = "smtp_security")]
    pub security: String,
    #[serde(default)]
    pub usuario: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "smtp_from_nombre")]
    pub from_nombre: String,
    #[serde(default)]
    pub from_email: String,
}
fn smtp_port() -> u16 { 587 }
fn smtp_security() -> String { "starttls".into() }
fn smtp_from_nombre() -> String { "JarStore".into() }

impl SmtpConfig {
    /// ¿Lista para enviar? (activa y con host y remitente)
    pub fn usable(&self) -> bool {
        self.activo && !self.host.trim().is_empty() && !self.from_email.trim().is_empty()
    }
}
