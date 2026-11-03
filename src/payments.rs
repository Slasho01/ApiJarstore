//! Integración con Mercado Pago (Checkout Pro).
//!
//! Variables: MP_ACCESS_TOKEN (obligatoria para activarlo), MP_WEBHOOK_SECRET (recomendada),
//! API_PUBLIC_URL (URL pública https de la API para recibir el webhook).
//! El estado de un pago SIEMPRE se consulta a la API de Mercado Pago: nunca se confía
//! en lo que llega por el navegador o en el cuerpo del webhook.
use crate::errors::ApiError;
use crate::models::orden::Orden;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

#[derive(Clone)]
pub struct MercadoPago {
    token: String,
    webhook_secret: Option<String>,
    api_base: String,
    api_public_url: String,
    frontend_url: String,
    http: reqwest::Client,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Preferencia {
    pub id: String,
    pub init_point: String,
    #[serde(default)]
    pub sandbox_init_point: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct MpPago {
    pub id: i64,
    pub status: String,
    #[serde(default)]
    pub external_reference: Option<String>,
    pub transaction_amount: f64,
    pub currency_id: String,
    #[serde(default)]
    pub payment_method_id: Option<String>,
}

impl MercadoPago {
    pub fn from_env(frontend_url: &str) -> Option<Self> {
        let token = std::env::var("MP_ACCESS_TOKEN").ok().filter(|t| !t.trim().is_empty())?;
        let api_public_url = std::env::var("API_PUBLIC_URL").unwrap_or_else(|_| frontend_url.to_string());
        if !api_public_url.starts_with("https://") {
            log::warn!("Mercado Pago: API_PUBLIC_URL ({api_public_url}) no es https; el webhook no llegará (en desarrollo el pago se sincroniza al volver del checkout)");
        }
        log::info!("Mercado Pago activado{}", if token.starts_with("TEST-") { " (credenciales de prueba)" } else { "" });
        Some(MercadoPago {
            token,
            webhook_secret: std::env::var("MP_WEBHOOK_SECRET").ok().filter(|s| !s.is_empty()),
            api_base: std::env::var("MP_API_BASE").unwrap_or_else(|_| "https://api.mercadopago.com".into()),
            api_public_url: api_public_url.trim_end_matches('/').to_string(),
            frontend_url: frontend_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        })
    }

    /// Crea la preferencia de pago para una orden (un ítem con el total ya calculado por la tienda).
    pub async fn create_preference(&self, orden: &Orden, tienda: &str, moneda_base: &str) -> Result<Preferencia, ApiError> {
        let short = orden.id.to_string()[..8].to_uppercase();
        let back = |estado: &str| format!("{}/pedido/{}?pago={estado}", self.frontend_url, orden.id);
        let body = serde_json::json!({
            "items": [{
                "id": orden.id,
                "title": format!("Pedido #{short} - {tienda}"),
                "quantity": 1,
                "currency_id": moneda_base,
                "unit_price": orden.total,
            }],
            "payer": { "email": orden.email_comprador, "name": orden.nombre_comprador },
            "external_reference": orden.id,
            "metadata": { "orden_id": orden.id },
            "back_urls": { "success": back("aprobado"), "pending": back("pendiente"), "failure": back("rechazado") },
            "auto_return": "approved",
            "notification_url": format!("{}/api/v2/pagos/mercadopago/webhook", self.api_public_url),
            "statement_descriptor": tienda.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ').take(22).collect::<String>(),
            "binary_mode": false,
        });
        let res = self.http.post(format!("{}/checkout/preferences", self.api_base))
            .bearer_auth(&self.token)
            .header("X-Idempotency-Key", format!("pref-{}-{}", orden.id, orden.updated_at.and_utc().timestamp()))
            .json(&body).send().await
            .map_err(|e| ApiError::internal(format!("Mercado Pago no respondió: {e}")))?;
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ApiError::internal(format!("Mercado Pago rechazó la preferencia ({status}): {text}")));
        }
        serde_json::from_str(&text).map_err(|e| ApiError::internal(format!("Respuesta de Mercado Pago inválida: {e}")))
    }

    pub async fn get_payment(&self, id: &str) -> Result<MpPago, ApiError> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            return Err(ApiError::bad_request("Identificador de pago inválido"));
        }
        let res = self.http.get(format!("{}/v1/payments/{id}", self.api_base)).bearer_auth(&self.token).send().await
            .map_err(|e| ApiError::internal(format!("Mercado Pago no respondió: {e}")))?;
        let status = res.status();
        if status.as_u16() == 404 {
            return Err(ApiError::not_found("Pago no encontrado en Mercado Pago"));
        }
        let text = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ApiError::internal(format!("Error consultando el pago ({status}): {text}")));
        }
        serde_json::from_str(&text).map_err(|e| ApiError::internal(format!("Pago de Mercado Pago inválido: {e}")))
    }

    /// Valida el header `x-signature` ("ts=...,v1=...") del webhook.
    /// Si no hay MP_WEBHOOK_SECRET configurado, se acepta (igual se consulta el pago a la API).
    pub fn verify_webhook(&self, x_signature: Option<&str>, x_request_id: Option<&str>, data_id: &str) -> bool {
        let Some(secret) = &self.webhook_secret else { return true };
        verify_signature(secret, x_signature, x_request_id, data_id)
    }
}

pub fn verify_signature(secret: &str, x_signature: Option<&str>, x_request_id: Option<&str>, data_id: &str) -> bool {
    let Some(sig) = x_signature else { return false };
    let (mut ts, mut v1) = (None, None);
    for part in sig.split(',') {
        match part.trim().split_once('=') {
            Some(("ts", v)) => ts = Some(v.trim()),
            Some(("v1", v)) => v1 = Some(v.trim()),
            _ => {}
        }
    }
    let (Some(ts), Some(v1)) = (ts, v1) else { return false };
    let id = if data_id.chars().all(|c| c.is_ascii_alphanumeric()) { data_id.to_lowercase() } else { data_id.to_string() };
    let mut manifest = String::new();
    if !id.is_empty() { manifest.push_str(&format!("id:{id};")); }
    if let Some(r) = x_request_id.filter(|r| !r.is_empty()) { manifest.push_str(&format!("request-id:{r};")); }
    manifest.push_str(&format!("ts:{ts};"));
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC acepta cualquier largo");
    mac.update(manifest.as_bytes());
    let expected: String = mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect();
    // comparación en tiempo constante
    expected.len() == v1.len() && expected.bytes().zip(v1.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn firma_webhook() {
        let secret = "secreto";
        let manifest = "id:123456;request-id:abc-1;ts:1700000000;";
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(manifest.as_bytes());
        let v1: String = mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let header = format!("ts=1700000000,v1={v1}");
        assert!(verify_signature(secret, Some(&header), Some("abc-1"), "123456"));
        assert!(!verify_signature(secret, Some(&header), Some("otro"), "123456"));
        assert!(!verify_signature(secret, Some("ts=1,v1=00"), Some("abc-1"), "123456"));
        assert!(!verify_signature(secret, None, None, "123456"));
    }
}
