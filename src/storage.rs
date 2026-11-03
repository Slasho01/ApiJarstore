//! Almacenamiento de imágenes subidas.
//!
//! - **Local** (por defecto): se optimizan en el servidor -> WebP en varios tamaños,
//!   guardadas en `UPLOAD_DIR` y servidas en `/uploads/...` con caché de 1 año.
//! - **Cloudinary** (si se define `CLOUDINARY_URL=cloudinary://API_KEY:API_SECRET@CLOUD_NAME`):
//!   se sube el original y los tamaños se entregan con transformaciones
//!   `f_auto,q_auto` (AVIF/WebP según el navegador).
use crate::errors::ApiError;
use crate::models::products::ImagenVariante;
use crate::services::common::slugify;
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageReader};
use sha1::{Digest, Sha1};
use std::io::Cursor;
use std::path::PathBuf;

/// Anchos que se generan (nunca se agranda una imagen más pequeña).
pub const WIDTHS: [u32; 4] = [320, 640, 1024, 1600];
pub const MAX_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_DOC_BYTES: usize = 15 * 1024 * 1024;
const MAX_SIDE: u32 = 8000;

#[derive(Clone)]
pub enum Storage {
    Local { dir: PathBuf },
    Cloudinary { cloud: String, key: String, secret: String, folder: String, api_base: String, http: reqwest::Client },
}

pub struct Stored {
    pub proveedor: &'static str,
    pub clave: String,
    pub url: String,
    pub variantes: Vec<ImagenVariante>,
    pub ancho: u32,
    pub alto: u32,
    pub bytes: i64,
}

impl Storage {
    pub fn from_env() -> Result<Self, String> {
        if let Ok(url) = std::env::var("CLOUDINARY_URL") {
            if !url.trim().is_empty() {
                let rest = url.trim().strip_prefix("cloudinary://").ok_or("CLOUDINARY_URL debe empezar con cloudinary://")?;
                let (cred, cloud) = rest.split_once('@').ok_or("CLOUDINARY_URL inválida (falta @CLOUD_NAME)")?;
                let (key, secret) = cred.split_once(':').ok_or("CLOUDINARY_URL inválida (falta API_KEY:API_SECRET)")?;
                log::info!("Imágenes: Cloudinary (cloud '{cloud}')");
                return Ok(Storage::Cloudinary {
                    cloud: cloud.to_string(),
                    key: key.to_string(),
                    secret: secret.to_string(),
                    folder: std::env::var("CLOUDINARY_FOLDER").unwrap_or_else(|_| "jarstore".into()),
                    api_base: std::env::var("CLOUDINARY_API_BASE").unwrap_or_else(|_| "https://api.cloudinary.com".into()),
                    http: reqwest::Client::new(),
                });
            }
        }
        let dir = PathBuf::from(std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".into()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear UPLOAD_DIR ({}): {e}", dir.display()))?;
        log::info!("Imágenes: almacenamiento local en {}", dir.display());
        Ok(Storage::Local { dir })
    }

    pub fn local_dir(&self) -> Option<&PathBuf> {
        match self { Storage::Local { dir } => Some(dir), _ => None }
    }

    pub fn provider(&self) -> &'static str {
        match self { Storage::Local { .. } => "local", Storage::Cloudinary { .. } => "cloudinary" }
    }

    /// Valida, optimiza y guarda una imagen. `hint` se usa para el nombre del archivo (SEO).
    pub async fn store(&self, data: Vec<u8>, hint: &str) -> Result<Stored, ApiError> {
        if data.len() > MAX_BYTES {
            return Err(ApiError::bad_request("La imagen supera los 10 MB"));
        }
        let name = format!("{}-{}", slugify(hint).chars().take(60).collect::<String>(), &uuid::Uuid::new_v4().simple().to_string()[..8]);
        match self {
            Storage::Local { dir } => {
                let dir = dir.clone();
                actix_web::web::block(move || store_local(&dir, &data, &name)).await?
            }
            Storage::Cloudinary { cloud, key, secret, folder, api_base, http } => {
                // Validar que sea una imagen real antes de subirla
                let probe = data.clone();
                let (w, h) = actix_web::web::block(move || decode(&probe).map(|i| i.dimensions())).await??;
                let timestamp = chrono::Utc::now().timestamp().to_string();
                let params = [("folder", folder.as_str()), ("public_id", name.as_str()), ("timestamp", timestamp.as_str())];
                let form = reqwest::multipart::Form::new()
                    .part("file", reqwest::multipart::Part::bytes(data.clone()).file_name(format!("{name}.img")))
                    .text("api_key", key.clone())
                    .text("folder", folder.clone())
                    .text("public_id", name.clone())
                    .text("timestamp", timestamp.clone())
                    .text("signature", cloudinary_signature(&params, secret));
                let res = http.post(format!("{api_base}/v1_1/{cloud}/image/upload")).multipart(form).send().await
                    .map_err(|e| ApiError::internal(format!("Cloudinary no respondió: {e}")))?;
                let status = res.status();
                let body: serde_json::Value = res.json().await.map_err(|e| ApiError::internal(format!("Respuesta de Cloudinary inválida: {e}")))?;
                if !status.is_success() {
                    return Err(ApiError::internal(format!("Cloudinary rechazó la imagen ({status}): {body}")));
                }
                let public_id = body["public_id"].as_str().unwrap_or_default().to_string();
                let version = body["version"].as_i64().unwrap_or(1);
                let url_for = |w: u32| format!("https://res.cloudinary.com/{cloud}/image/upload/f_auto,q_auto,c_limit,w_{w}/v{version}/{public_id}");
                let variantes = widths_for(w).into_iter().map(|w| ImagenVariante { w, url: url_for(w), mime: None }).collect::<Vec<_>>();
                Ok(Stored {
                    proveedor: "cloudinary",
                    url: variantes.last().map(|v| v.url.clone()).unwrap_or_else(|| url_for(w)),
                    clave: public_id,
                    variantes,
                    ancho: w,
                    alto: h,
                    bytes: body["bytes"].as_i64().unwrap_or(data.len() as i64),
                })
            }
        }
    }

    /// Guarda un documento (boleta/factura): PDF o imagen, tal cual (sin redimensionar).
    pub async fn store_document(&self, data: Vec<u8>, hint: &str) -> Result<Stored, ApiError> {
        if data.len() > MAX_DOC_BYTES {
            return Err(ApiError::bad_request("El documento supera los 15 MB"));
        }
        let is_pdf = data.starts_with(b"%PDF");
        let ext = if is_pdf {
            "pdf"
        } else {
            // Si no es PDF, debe ser una imagen válida
            let probe = data.clone();
            let fmt = actix_web::web::block(move || image::guess_format(&probe)).await?
                .map_err(|_| ApiError::bad_request("El documento debe ser un PDF o una imagen (JPG, PNG, WebP)"))?;
            match fmt {
                image::ImageFormat::Jpeg => "jpg",
                image::ImageFormat::Png => "png",
                image::ImageFormat::WebP => "webp",
                image::ImageFormat::Gif => "gif",
                _ => return Err(ApiError::bad_request("Formato de documento no soportado")),
            }
        };
        let mime = match ext { "pdf" => "application/pdf", "jpg" => "image/jpeg", "png" => "image/png", "webp" => "image/webp", _ => "image/gif" };
        let name = format!("{}-{}", slugify(hint).chars().take(60).collect::<String>(), &uuid::Uuid::new_v4().simple().to_string()[..8]);
        match self {
            Storage::Local { dir } => {
                let dir = dir.clone();
                let name = name.clone();
                let ext = ext.to_string();
                let mime = mime.to_string();
                actix_web::web::block(move || {
                    let sub = chrono::Utc::now().format("docs/%Y/%m").to_string();
                    let folder = dir.join(&sub);
                    std::fs::create_dir_all(&folder).map_err(|e| ApiError::internal(format!("No se pudo crear {}: {e}", folder.display())))?;
                    let file = format!("{name}.{ext}");
                    std::fs::write(folder.join(&file), &data).map_err(|e| ApiError::internal(format!("No se pudo guardar el documento: {e}")))?;
                    let url = format!("/uploads/{sub}/{file}");
                    Ok::<Stored, ApiError>(Stored {
                        proveedor: "local", clave: format!("{sub}/{name}"), url: url.clone(),
                        variantes: vec![ImagenVariante { w: 0, url, mime: Some(mime) }],
                        ancho: 0, alto: 0, bytes: data.len() as i64,
                    })
                }).await?
            }
            Storage::Cloudinary { cloud, key, secret, folder, api_base, http } => {
                let timestamp = chrono::Utc::now().timestamp().to_string();
                let public_id = format!("{name}.{ext}");
                let params = [("folder", folder.as_str()), ("public_id", public_id.as_str()), ("timestamp", timestamp.as_str())];
                let form = reqwest::multipart::Form::new()
                    .part("file", reqwest::multipart::Part::bytes(data.clone()).file_name(public_id.clone()))
                    .text("api_key", key.clone())
                    .text("folder", folder.clone())
                    .text("public_id", public_id.clone())
                    .text("timestamp", timestamp.clone())
                    .text("signature", cloudinary_signature(&params, secret));
                // /auto/upload entrega tanto imágenes como PDF (raw)
                let res = http.post(format!("{api_base}/v1_1/{cloud}/auto/upload")).multipart(form).send().await
                    .map_err(|e| ApiError::internal(format!("Cloudinary no respondió: {e}")))?;
                let status = res.status();
                let body: serde_json::Value = res.json().await.map_err(|e| ApiError::internal(format!("Respuesta de Cloudinary inválida: {e}")))?;
                if !status.is_success() {
                    return Err(ApiError::internal(format!("Cloudinary rechazó el documento ({status}): {body}")));
                }
                let url = body["secure_url"].as_str().unwrap_or_default().to_string();
                Ok(Stored {
                    proveedor: "cloudinary",
                    clave: body["public_id"].as_str().unwrap_or_default().to_string(),
                    variantes: vec![ImagenVariante { w: 0, url: url.clone(), mime: Some(mime.to_string()) }],
                    url, ancho: 0, alto: 0, bytes: body["bytes"].as_i64().unwrap_or(data.len() as i64),
                })
            }
        }
    }

    /// Borra los archivos de una subida (si falla, sólo se registra en el log).
    pub async fn delete(&self, proveedor: &str, clave: &str, variantes: &[ImagenVariante]) {
        match (self, proveedor) {
            (Storage::Local { dir }, "local") => {
                for v in variantes {
                    if let Some(rel) = v.url.strip_prefix("/uploads/") {
                        let path = dir.join(rel);
                        if path.starts_with(dir) {
                            let _ = std::fs::remove_file(path);
                        }
                    }
                }
                log::info!("Imagen local eliminada: {clave}");
            }
            (Storage::Cloudinary { cloud, key, secret, api_base, http, .. }, "cloudinary") => {
                let timestamp = chrono::Utc::now().timestamp().to_string();
                let sig = cloudinary_signature(&[("public_id", clave), ("timestamp", &timestamp)], secret);
                let r = http.post(format!("{api_base}/v1_1/{cloud}/image/destroy"))
                    .form(&[("public_id", clave), ("timestamp", &timestamp), ("api_key", key), ("signature", &sig)])
                    .send().await;
                if let Err(e) = r {
                    log::warn!("No se pudo borrar {clave} de Cloudinary: {e}");
                }
            }
            _ => log::warn!("La imagen {clave} pertenece a otro almacenamiento ({proveedor}); no se borra"),
        }
    }
}

/// Firma de Cloudinary: SHA-1 de los parámetros ordenados "a=1&b=2" + API secret.
pub fn cloudinary_signature(params: &[(&str, &str)], secret: &str) -> String {
    let mut p: Vec<_> = params.iter().filter(|(_, v)| !v.is_empty()).collect();
    p.sort_by_key(|(k, _)| *k);
    let joined = p.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
    Sha1::digest(format!("{joined}{secret}").as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn widths_for(original: u32) -> Vec<u32> {
    let mut w: Vec<u32> = WIDTHS.iter().copied().filter(|w| *w <= original).collect();
    if w.is_empty() || (original < 1600 && *w.last().unwrap() < original && original - w.last().unwrap() > 100) {
        w.push(original.min(1600));
    }
    w
}

fn decode(data: &[u8]) -> Result<DynamicImage, ApiError> {
    let reader = ImageReader::new(Cursor::new(data)).with_guessed_format()
        .map_err(|_| ApiError::bad_request("No se pudo leer el archivo"))?;
    match reader.format() {
        Some(image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::WebP | image::ImageFormat::Gif) => {}
        _ => return Err(ApiError::bad_request("Formato no soportado: usa JPG, PNG, WebP o GIF")),
    }
    let mut reader = reader;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|e| ApiError::bad_request(format!("La imagen está dañada o es demasiado grande: {e}")))
}

fn store_local(dir: &std::path::Path, data: &[u8], name: &str) -> Result<Stored, ApiError> {
    let img = decode(data)?;
    let (w0, h0) = img.dimensions();
    let sub = chrono::Utc::now().format("%Y/%m").to_string();
    let folder = dir.join(&sub);
    std::fs::create_dir_all(&folder).map_err(|e| ApiError::internal(format!("No se pudo crear {}: {e}", folder.display())))?;
    let mut variantes = vec![];
    let mut total = 0i64;
    for w in widths_for(w0) {
        let resized = if w >= w0 { img.clone() } else { img.resize(w, u32::MAX, FilterType::Lanczos3) };
        let rgba = resized.to_rgba8();
        let encoded = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height()).encode(82.0);
        let file = format!("{name}-{w}.webp");
        std::fs::write(folder.join(&file), &*encoded).map_err(|e| ApiError::internal(format!("No se pudo guardar la imagen: {e}")))?;
        total += encoded.len() as i64;
        variantes.push(ImagenVariante { w: rgba.width(), url: format!("/uploads/{sub}/{file}"), mime: Some("image/webp".into()) });
    }
    Ok(Stored {
        proveedor: "local",
        clave: format!("{sub}/{name}"),
        url: variantes.last().map(|v| v.url.clone()).unwrap_or_default(),
        variantes,
        ancho: w0,
        alto: h0,
        bytes: total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firma_cloudinary_ejemplo_documentado() {
        // Ejemplo de la documentación de Cloudinary ("Generating authentication signatures")
        let sig = cloudinary_signature(
            &[("eager", "w_400,h_300,c_pad|w_260,h_200,c_crop"), ("public_id", "sample_image"), ("timestamp", "1315060510")],
            "abcd",
        );
        assert_eq!(sig, "bfd09f95f331f558cbd1320e67aa8d488770583e");
    }

    #[test]
    fn genera_webp_en_varios_tamanos() {
        let dir = std::env::temp_dir().join(format!("jarstore-test-{}", uuid::Uuid::new_v4()));
        let img = image::RgbImage::from_fn(1200, 800, |x, y| image::Rgb([(x % 255) as u8, (y % 255) as u8, 128]));
        let mut png = Vec::new();
        DynamicImage::ImageRgb8(img).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let s = store_local(&dir, &png, "notebook-pro").unwrap();
        let ws: Vec<u32> = s.variantes.iter().map(|v| v.w).collect();
        assert_eq!(ws, vec![320, 640, 1024, 1200]);
        assert!(s.url.ends_with("-1200.webp") && s.url.starts_with("/uploads/"));
        for v in &s.variantes {
            let path = dir.join(v.url.trim_start_matches("/uploads/"));
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(&bytes[8..12], b"WEBP");
        }
        assert!(decode(b"no es una imagen").is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
