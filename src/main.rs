mod auth;
mod config;
mod courier;
mod controller;
mod db;
mod errors;
mod jobs;
mod mailer;
mod melihub;
mod payments;
mod models;
mod routes;
mod schema;
mod seo;
mod services;
mod storage;

use crate::auth::LoginLimiter;
use crate::config::Config;
use crate::errors::ApiError;
use actix_cors::Cors;
use actix_web::middleware::{DefaultHeaders, Logger, NormalizePath, TrailingSlash};
use actix_web::{error, http, web, App, HttpServer};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let config = Config::from_env().unwrap_or_else(|e| {
        eprintln!("Error de configuración: {e}");
        std::process::exit(1);
    });
    let pool = db::establish_pool(&config.database_url, config.db_pool_size).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    if let Err(e) = db::run_migrations(&pool) {
        eprintln!("{e}");
        std::process::exit(1);
    }

    let addr = (config.host.clone(), config.port);
    log::info!("JarStore API escuchando en http://{}:{}", addr.0, addr.1);

    let config_data = web::Data::new(config);
    let pool_data = web::Data::new(pool);
    let limiter = web::Data::new(LoginLimiter::new());
    let mailer_data = web::Data::new(mailer::Mailer::new(pool_data.get_ref().clone()));
    let storage = storage::Storage::from_env().unwrap_or_else(|e| {
        eprintln!("Error de configuración de imágenes: {e}");
        std::process::exit(1);
    });
    let upload_dir = storage.local_dir().cloned();
    let storage_data = web::Data::new(storage);
    let payments = web::Data::new(controller::order_controller::Payments {
        mp: payments::MercadoPago::from_env(&config_data.frontend_url),
    });
    let courier_data = web::Data::new(courier::Courier::from_env());
    let melihub_data = web::Data::new(melihub::Melihub::from_env());
    let site = web::Data::new(seo::Site::from_env(&config_data.frontend_url));
    jobs::start(pool_data.get_ref().clone(), mailer_data.get_ref().clone(), config_data.frontend_url.clone());

    HttpServer::new(move || {
        App::new()
            .app_data(config_data.clone())
            .app_data(pool_data.clone())
            .app_data(limiter.clone())
            .app_data(mailer_data.clone())
            .app_data(storage_data.clone())
            .app_data(payments.clone())
            .app_data(courier_data.clone())
            .app_data(melihub_data.clone())
            .app_data(site.clone())
            // Errores de JSON / query / path mal formados también salen como JSON
            .app_data(web::JsonConfig::default().limit(1 << 20).error_handler(|err, _| {
                error::Error::from(ApiError::bad_request(format!("JSON inválido: {err}")))
            }))
            .app_data(web::QueryConfig::default().error_handler(|err, _| {
                error::Error::from(ApiError::bad_request(format!("Parámetros inválidos: {err}")))
            }))
            .app_data(web::PathConfig::default().error_handler(|err, _| {
                error::Error::from(ApiError::bad_request(format!("Ruta inválida: {err}")))
            }))
            .wrap(security_headers())
            .wrap(cors(&config_data))
            .wrap(NormalizePath::new(TrailingSlash::Trim))
            .wrap(Logger::new("%a \"%r\" %s %Dms"))
            .configure(routes::configure_routes)
            .configure(|cfg| {
                // Imágenes subidas (nombres únicos => caché de 1 año)
                if let Some(dir) = &upload_dir {
                    cfg.service(
                        web::scope("/uploads")
                            .wrap(DefaultHeaders::new().add(("Cache-Control", "public, max-age=31536000, immutable")))
                            .service(actix_files::Files::new("", dir.clone())),
                    );
                }
            })
            .route("/sitemap.xml", web::get().to(seo::sitemap))
            .route("/robots.txt", web::get().to(seo::robots))
            // Frontend + SEO (o 404 JSON si el frontend no se sirve desde aquí)
            .default_service(web::get().to(seo::frontend))
    })
    .bind(addr)?
    .run()
    .await
}

/// Cabeceras de seguridad en todas las respuestas (DefaultHeaders no pisa las ya presentes,
/// p. ej. Cache-Control por ruta). No se incluye un CSP de script/style/img estricto porque
/// la tienda usa estilos en línea e imágenes de productos desde hosts externos; se protege
/// contra clickjacking (frame-ancestors) e inyección de <base>/formularios.
fn security_headers() -> DefaultHeaders {
    DefaultHeaders::new()
        .add(("X-Content-Type-Options", "nosniff"))
        .add(("X-Frame-Options", "DENY"))
        .add(("Referrer-Policy", "strict-origin-when-cross-origin"))
        .add(("Cross-Origin-Opener-Policy", "same-origin"))
        .add(("Content-Security-Policy", "frame-ancestors 'none'; base-uri 'self'; form-action 'self'"))
        // Ignorada por el navegador sobre http; activa HSTS una vez detrás de HTTPS.
        .add(("Strict-Transport-Security", "max-age=31536000; includeSubDomains"))
}

fn cors(config: &Config) -> Cors {
    let base = Cors::default()
        .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
        .allowed_headers(vec![http::header::AUTHORIZATION, http::header::CONTENT_TYPE])
        .max_age(3600);
    if config.cors_origins.is_empty() {
        base.allow_any_origin()
    } else {
        config.cors_origins.iter().fold(base, |c, o| c.allowed_origin(o))
    }
}
