use std::env;

/// Configuración leída una sola vez al arrancar. Si falta algo obligatorio el
/// servidor no arranca (mejor fallar al inicio que en mitad de una petición).
#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_ttl_seconds: u64,
    pub host: String,
    pub port: u16,
    pub cors_origins: Vec<String>,
    pub db_pool_size: u32,
    /// URL pública del frontend (para enlaces en correos)
    pub frontend_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let database_url = env::var("DATABASE_URL").map_err(|_| "DATABASE_URL no está definida".to_string())?;

        // JWT_SECRET es el nombre nuevo; WT_SECRET_KEY se mantiene por compatibilidad.
        let jwt_secret = env::var("JWT_SECRET")
            .or_else(|_| env::var("WT_SECRET_KEY"))
            .map_err(|_| "JWT_SECRET no está definida".to_string())?;
        if jwt_secret.len() < 16 {
            return Err("JWT_SECRET debe tener al menos 16 caracteres (recomendado 32+)".into());
        }

        let parse = |name: &str, default: &str| env::var(name).unwrap_or_else(|_| default.to_string());

        Ok(Config {
            database_url,
            jwt_secret,
            jwt_ttl_seconds: parse("JWT_TTL_SECONDS", "7200")
                .parse()
                .map_err(|_| "JWT_TTL_SECONDS inválido")?,
            host: parse("HOST", "127.0.0.1"),
            port: parse("PORT", "8080").parse().map_err(|_| "PORT inválido")?,
            cors_origins: parse("CORS_ORIGINS", "")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            frontend_url: parse("FRONTEND_URL", "http://localhost:5173").trim_end_matches('/').to_string(),
            db_pool_size: parse("DB_POOL_SIZE", "10")
                .parse()
                .map_err(|_| "DB_POOL_SIZE inválido")?,
        })
    }
}
