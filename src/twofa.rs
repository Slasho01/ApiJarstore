//! 2FA TOTP (Google Authenticator / Authy). SHA1, 6 dígitos, ventana de 30s.
//! El secreto se guarda cifrado en reposo (ver `crypto`).

use crate::errors::{ApiError, ApiResult};
use totp_rs::{Algorithm, Secret, TOTP};

pub fn generar_secret() -> String {
    Secret::generate_secret().to_encoded().to_string()
}

fn totp_de(secret_b32: &str, cuenta: &str) -> ApiResult<TOTP> {
    let bytes = Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .map_err(|_| ApiError::internal("Secreto 2FA inválido"))?;
    Ok(TOTP::new_unchecked(Algorithm::SHA1, 6, 1, 30, bytes, Some("JarStore".to_string()), cuenta.to_string()))
}

pub fn url(secret_b32: &str, cuenta: &str) -> ApiResult<String> {
    Ok(totp_de(secret_b32, cuenta)?.get_url())
}

pub fn qr(secret_b32: &str, cuenta: &str) -> ApiResult<String> {
    totp_de(secret_b32, cuenta)?
        .get_qr_base64()
        .map_err(|e| ApiError::internal(format!("No se pudo generar el QR: {e}")))
}

pub fn verificar(secret_b32: &str, codigo: &str) -> bool {
    match totp_de(secret_b32, "user") {
        Ok(t) => t.check_current(codigo.trim()).unwrap_or(false),
        Err(_) => false,
    }
}
