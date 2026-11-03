//! Cotización de envíos con **Blue Express** (API BX-Pricing), para Chile.
//!
//! Blue Express entrega estas credenciales al contratar la cuenta de empresa;
//! se cargan por variables de entorno:
//!   BLUEEXPRESS_TOKEN           -> header `BX-TOKEN`           (obligatoria para activarlo)
//!   BLUEEXPRESS_USERCODE        -> header `BX-USERCODE`
//!   BLUEEXPRESS_CLIENT_ACCOUNT  -> header `BX-CLIENT_ACCOUNT`
//!   BLUEEXPRESS_API_BASE        -> URL base de la API (opcional; útil para pruebas)
//!
//! La tarifa de Blue Express se calcula con la comuna de origen y destino, el
//! peso y las dimensiones (usa el MAYOR entre el peso real y el volumétrico).
//!
//! NOTA DE INTEGRACIÓN: el contrato exacto de BX-Pricing (ruta y nombres de los
//! campos JSON) puede variar según la versión que Blue Express habilite para tu
//! cuenta. Todo el mapeo está aislado en este archivo (constante `RATES_PATH`,
//! structs `BxRequest`/`BxResponse`), así que para ajustarlo sólo se toca aquí.
//! Si el courier falla o no está configurado, la tienda cae automáticamente a
//! los métodos de envío manuales (ver `ordenes_services::shipping`).

use crate::errors::{ApiError, ApiResult};
use serde::{Deserialize, Serialize};

/// Ruta del cotizador dentro de la API de Blue Express.
const RATES_PATH: &str = "/rating/api/v1/rates";
/// Base por defecto (ambiente productivo de Blue Express).
const DEFAULT_BASE: &str = "https://bx-pricing.bluex.cl";
/// Factor volumétrico (cm³ por kg) que usa Blue Express.
pub const FACTOR_VOLUMETRICO: i64 = 4000;

#[derive(Clone)]
pub struct Courier {
    bx: Option<BlueExpress>,
}

#[derive(Clone)]
struct BlueExpress {
    token: String,
    usercode: String,
    account: String,
    base: String,
}

/// Una opción de servicio devuelta por el courier (ya normalizada).
#[derive(Debug, Clone)]
pub struct OpcionCourier {
    pub codigo: String,
    pub nombre: String,
    /// Precio en pesos chilenos (moneda base)
    pub precio: i64,
    pub dias: Option<i32>,
}

// --- Contrato con BX-Pricing (ajustar aquí si cambia) ---

#[derive(Serialize)]
struct BxPackage {
    weight: String, // kg
    height: i64,    // cm
    width: i64,
    length: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BxRequest {
    origin_county_code: String,
    destination_county_code: String,
    package: BxPackage,
    declared_worth: String,
    /// 3 = entrega a domicilio (según documentación de Blue Express)
    product_type: i32,
}

#[derive(Deserialize)]
struct BxResponse {
    #[serde(default)]
    data: Option<BxData>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BxData {
    #[serde(default, alias = "courierServiceOptions")]
    service_list: Vec<BxService>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BxService {
    #[serde(default)]
    service_type_code: Option<serde_json::Value>,
    #[serde(default, alias = "serviceName")]
    service_description: Option<String>,
    /// El precio puede venir como número o como texto
    #[serde(default, alias = "totalPrice")]
    service_value: Option<serde_json::Value>,
    #[serde(default)]
    delivery_time: Option<i32>,
}

impl Courier {
    pub fn from_env() -> Self {
        let bx = std::env::var("BLUEEXPRESS_TOKEN").ok().filter(|t| !t.trim().is_empty()).map(|token| {
            let base = std::env::var("BLUEEXPRESS_API_BASE").ok().filter(|b| !b.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_BASE.to_string());
            log::info!("Blue Express activado (cotización de envíos para Chile)");
            BlueExpress {
                token,
                usercode: std::env::var("BLUEEXPRESS_USERCODE").unwrap_or_default(),
                account: std::env::var("BLUEEXPRESS_CLIENT_ACCOUNT").unwrap_or_default(),
                base: base.trim_end_matches('/').to_string(),
            }
        });
        Courier { bx }
    }

    pub fn enabled(&self) -> bool {
        self.bx.is_some()
    }

    /// Cotiza un envío. `peso_g` es el peso real total; se usa el mayor entre
    /// ese y el volumétrico. Devuelve las opciones de servicio disponibles.
    pub fn quote(&self, origen: &str, destino: &str, peso_g: i64, volumen_cm3: i64, valor: i64) -> ApiResult<Vec<OpcionCourier>> {
        let bx = self.bx.as_ref().ok_or_else(|| ApiError::bad_request("El courier no está configurado"))?;
        let origen = origen.trim();
        let destino = destino.trim();
        if origen.is_empty() {
            return Err(ApiError::bad_request("Falta la comuna de origen del courier (configúrala en Envíos)"));
        }
        if destino.is_empty() {
            return Err(ApiError::bad_request("Elige la comuna de destino"));
        }
        let peso_facturable_g = peso_g.max(volumen_cm3 * 1000 / FACTOR_VOLUMETRICO).max(1);
        let kg = (peso_facturable_g as f64 / 1000.0).max(0.1);
        // Dimensiones de la caja equivalente (cúbica) a partir del volumen
        let lado = if volumen_cm3 > 0 { (volumen_cm3 as f64).cbrt().ceil() as i64 } else { 1 };
        let body = BxRequest {
            origin_county_code: origen.to_string(),
            destination_county_code: destino.to_string(),
            package: BxPackage { weight: format!("{kg:.2}"), height: lado, width: lado, length: lado.max(1) },
            declared_worth: valor.max(0).to_string(),
            product_type: 3,
        };
        // El cliente bloqueante se crea aquí (dentro del pool de hilos bloqueantes),
        // no en el arranque async, para no romper el runtime de tokio.
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .map_err(|e| ApiError::internal(format!("No se pudo crear el cliente HTTP: {e}")))?;
        let res = http.post(format!("{}{RATES_PATH}", bx.base))
            .header("BX-TOKEN", &bx.token)
            .header("BX-USERCODE", &bx.usercode)
            .header("BX-CLIENT_ACCOUNT", &bx.account)
            .json(&body)
            .send()
            .map_err(|e| ApiError::internal(format!("Blue Express no respondió: {e}")))?;
        let status = res.status();
        let text = res.text().unwrap_or_default();
        if !status.is_success() {
            return Err(ApiError::internal(format!("Blue Express rechazó la cotización ({status}): {text}")));
        }
        let parsed: BxResponse = serde_json::from_str(&text)
            .map_err(|e| ApiError::internal(format!("Respuesta de Blue Express no reconocida: {e}")))?;
        let opciones = parsed.data.map(|d| d.service_list).unwrap_or_default().into_iter().filter_map(|s| {
            let precio = valor_a_entero(s.service_value?)?;
            let codigo = match s.service_type_code {
                Some(serde_json::Value::String(s)) => s,
                Some(serde_json::Value::Number(n)) => n.to_string(),
                _ => "bx".to_string(),
            };
            Some(OpcionCourier {
                codigo: format!("bx-{codigo}"),
                nombre: s.service_description.unwrap_or_else(|| "Blue Express".to_string()),
                precio,
                dias: s.delivery_time,
            })
        }).collect();
        Ok(opciones)
    }
}

/// Acepta el precio como número entero, decimal o texto ("4890" o "4890.0").
fn valor_a_entero(v: serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().map(|f| f.round() as i64),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok().map(|f| f.round() as i64),
        _ => None,
    }
}
