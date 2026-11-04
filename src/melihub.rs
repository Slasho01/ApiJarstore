//! Cliente de **MeliHub**, el hub que centraliza las publicaciones de Mercado
//! Libre. JarStore lo consume como cliente delgado a través de su API pública
//! (autenticación por API-key en el header `X-API-Key`):
//!
//!   - `GET  {base}/api/pub/productos`  -> catálogo con el precio del canal JarStore.
//!   - `POST {base}/api/pub/ventas`     -> reporta una venta; MeliHub descuenta el
//!                                         stock y lo empuja a Mercado Libre.
//!
//! Se activa sólo si están definidas las variables de entorno:
//!   MELIHUB_URL       -> base de la API de MeliHub (ej. https://mi-melihub.ngrok-free.dev)
//!   MELIHUB_API_KEY   -> la API-key (canal JarStore) creada en MeliHub
//!
//! Si no está configurado, JarStore funciona igual (la integración queda inerte).
//! Todo el contrato con MeliHub vive aquí, para ajustarlo en un solo lugar.

use crate::errors::{ApiError, ApiResult};
use serde::Deserialize;
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub struct Melihub {
    inner: Option<Inner>,
}

#[derive(Clone)]
struct Inner {
    base: String,
    api_key: String,
}

/// Producto tal como lo expone MeliHub en `/api/pub/productos`.
#[derive(Debug, Clone, Deserialize)]
pub struct PublicProducto {
    pub id: Uuid,
    pub ml_item_id: String,
    pub titulo: String,
    pub precio: i64,
    #[serde(default)]
    pub moneda: String,
    pub stock: i32,
    pub estado: String,
    #[serde(default)]
    pub thumbnail: Option<String>,
}

impl Melihub {
    pub fn from_env() -> Self {
        let base = std::env::var("MELIHUB_URL")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty());
        let api_key = std::env::var("MELIHUB_API_KEY").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let inner = match (base, api_key) {
            (Some(base), Some(api_key)) => Some(Inner { base, api_key }),
            _ => None,
        };
        if inner.is_some() {
            log::info!("MeliHub configurado (integración de catálogo/stock activa)");
        }
        Melihub { inner }
    }

    pub fn enabled(&self) -> bool {
        self.inner.is_some()
    }

    fn require(&self) -> ApiResult<&Inner> {
        self.inner
            .as_ref()
            .ok_or_else(|| ApiError::bad_request("MeliHub no está configurado. Define MELIHUB_URL y MELIHUB_API_KEY en el servidor."))
    }

    /// Cliente HTTP bloqueante. Se crea dentro del hilo bloqueante (no en el
    /// arranque async) para no romper el runtime de tokio.
    fn http() -> ApiResult<reqwest::blocking::Client> {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| ApiError::internal(format!("No se pudo crear el cliente HTTP: {e}")))
    }

    /// Trae el catálogo publicado por MeliHub (con el precio del canal de la API-key).
    pub fn fetch_productos(&self) -> ApiResult<Vec<PublicProducto>> {
        let c = self.require()?;
        let res = Self::http()?
            .get(format!("{}/api/pub/productos", c.base))
            .header("X-API-Key", &c.api_key)
            .header("ngrok-skip-browser-warning", "1")
            .send()
            .map_err(|e| ApiError::internal(format!("MeliHub no respondió: {e}")))?;
        let status = res.status();
        let text = res.text().unwrap_or_default();
        if !status.is_success() {
            return Err(ApiError::internal(format!("MeliHub rechazó la consulta ({status}): {text}")));
        }
        serde_json::from_str(&text).map_err(|e| ApiError::internal(format!("Catálogo de MeliHub no reconocido: {e}")))
    }

    /// Reporta una venta a MeliHub (descuenta stock allí y lo empuja a Mercado Libre).
    /// Best-effort: devuelve error para que el llamador lo registre, sin frenar la venta.
    pub fn reportar_venta(&self, producto_id: Uuid, cantidad: i32) -> ApiResult<()> {
        let c = self.require()?;
        let res = Self::http()?
            .post(format!("{}/api/pub/ventas", c.base))
            .header("X-API-Key", &c.api_key)
            .header("ngrok-skip-browser-warning", "1")
            .json(&serde_json::json!({ "producto_id": producto_id, "cantidad": cantidad }))
            .send()
            .map_err(|e| ApiError::internal(format!("MeliHub no respondió: {e}")))?;
        let status = res.status();
        if !status.is_success() {
            let text = res.text().unwrap_or_default();
            return Err(ApiError::internal(format!("MeliHub rechazó la venta ({status}): {text}")));
        }
        Ok(())
    }
}
