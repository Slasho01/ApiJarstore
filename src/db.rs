use crate::errors::{ApiError, ApiResult};
use actix_web::web;
use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

pub type DbPool = Pool<ConnectionManager<PgConnection>>;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

pub fn establish_pool(database_url: &str, max_size: u32) -> Result<DbPool, String> {
    let manager = ConnectionManager::<PgConnection>::new(database_url);
    Pool::builder()
        .max_size(max_size)
        .build(manager)
        .map_err(|e| format!("No se pudo crear el pool de conexiones: {e}"))
}

/// Aplica las migraciones pendientes (embebidas en el binario), así no hace
/// falta tener diesel_cli instalado para levantar la API.
pub fn run_migrations(pool: &DbPool) -> Result<(), String> {
    let mut conn = pool.get().map_err(|e| e.to_string())?;
    let applied = conn
        .run_pending_migrations(MIGRATIONS)
        .map_err(|e| format!("Error aplicando migraciones: {e}"))?;
    for m in applied {
        log::info!("Migración aplicada: {m}");
    }
    Ok(())
}

/// Ejecuta código de Diesel (que es síncrono) en el pool de hilos bloqueantes
/// de actix, para no congelar los workers async mientras se espera a la BD.
pub async fn run<F, T>(pool: &web::Data<DbPool>, f: F) -> ApiResult<T>
where
    F: FnOnce(&mut PgConnection) -> ApiResult<T> + Send + 'static,
    T: Send + 'static,
{
    let pool = pool.clone();
    web::block(move || {
        let mut conn = pool.get().map_err(ApiError::from)?;
        f(&mut conn)
    })
    .await?
}
