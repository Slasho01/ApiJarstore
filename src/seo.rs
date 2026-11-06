//! SEO en el servidor: la API sirve el frontend compilado (Frontend/dist) e inserta en el
//! HTML inicial los metadatos de cada página (title, description, canonical, Open Graph,
//! JSON-LD). Así buscadores y redes sociales ven precios, stock y títulos sin ejecutar JS.
//! También genera /sitemap.xml y /robots.txt.
use crate::db::{self, DbPool};
use crate::errors::{ApiError, ApiResult};
use crate::models::category::Categoria;
use crate::models::tienda::{MonedasConfig, TiendaConfig};
use crate::schema::{categorias, paginas, product_images, productos, resenas};
use crate::services::{category_services, product_services, tienda_services};
use actix_web::{http::header, web, HttpRequest, HttpResponse};
use diesel::prelude::*;
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct Site {
    pub dist: Option<PathBuf>,
    /// index.html en caché junto con su fecha de modificación: si se vuelve a
    /// compilar el frontend, se recarga solo (sin reiniciar la API).
    template: std::sync::RwLock<Option<(std::time::SystemTime, std::sync::Arc<String>)>>,
    pub public_url: String,
    /// Tags de analítica/verificación (GA4, Search Console, Meta Pixel), por env.
    analytics: String,
}

/// Deja solo caracteres seguros para un id (evita inyección vía variables de entorno).
fn safe_id(s: &str) -> String {
    s.trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect()
}

/// Construye los snippets de analítica desde variables de entorno (los que estén definidos).
fn build_analytics() -> String {
    const GA_TPL: &str = "<script async src=\"https://www.googletagmanager.com/gtag/js?id=__ID__\"></script>\n<script>window.dataLayer=window.dataLayer||[];function gtag(){dataLayer.push(arguments);}gtag('js',new Date());gtag('config','__ID__');</script>\n";
    const PIXEL_TPL: &str = "<script>!function(f,b,e,v,n,t,s){if(f.fbq)return;n=f.fbq=function(){n.callMethod?n.callMethod.apply(n,arguments):n.queue.push(arguments)};if(!f._fbq)f._fbq=n;n.push=n;n.loaded=!0;n.version='2.0';n.queue=[];t=b.createElement(e);t.async=!0;t.src=v;s=b.getElementsByTagName(e)[0];s.parentNode.insertBefore(t,s)}(window,document,'script','https://connect.facebook.net/en_US/fbevents.js');fbq('init','__ID__');fbq('track','PageView');</script>\n<noscript><img height=\"1\" width=\"1\" style=\"display:none\" src=\"https://www.facebook.com/tr?id=__ID__&ev=PageView&noscript=1\"/></noscript>\n";
    let mut h = String::new();
    if let Ok(v) = std::env::var("GOOGLE_SITE_VERIFICATION") {
        let v = safe_id(&v);
        if !v.is_empty() {
            h.push_str(&format!("<meta name=\"google-site-verification\" content=\"{v}\" />\n"));
        }
    }
    if let Ok(v) = std::env::var("GA_MEASUREMENT_ID") {
        let v = safe_id(&v);
        if !v.is_empty() {
            h.push_str(&GA_TPL.replace("__ID__", &v));
        }
    }
    if let Ok(v) = std::env::var("META_PIXEL_ID") {
        let v = safe_id(&v);
        if !v.is_empty() {
            h.push_str(&PIXEL_TPL.replace("__ID__", &v));
        }
    }
    h
}

impl Site {
    pub fn from_env(default_url: &str) -> Self {
        let public_url = std::env::var("PUBLIC_URL").unwrap_or_else(|_| default_url.to_string()).trim_end_matches('/').to_string();
        let dist = std::env::var("FRONTEND_DIST").ok().filter(|d| !d.is_empty()).map(PathBuf::from)
            .or_else(|| Some(PathBuf::from("../Frontend/dist")).filter(|p| p.join("index.html").exists()));
        match &dist {
            Some(d) if d.join("index.html").exists() => log::info!("Sirviendo el frontend desde {} (SEO en servidor, URL pública {public_url})", d.display()),
            Some(d) => log::warn!("FRONTEND_DIST={} no contiene index.html (ejecuta `npm run build`)", d.display()),
            None => log::info!("Frontend no servido por la API (define FRONTEND_DIST para producción)"),
        }
        let analytics = build_analytics();
        if !analytics.is_empty() {
            log::info!("Analítica/verificación activa (GA/Pixel/Search Console por env)");
        }
        Site { dist, template: std::sync::RwLock::new(None), public_url, analytics }
    }

    /// index.html actual (se relee si cambió en disco).
    fn template(&self) -> Option<std::sync::Arc<String>> {
        let path = self.dist.as_ref()?.join("index.html");
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
        if let Some((t, html)) = self.template.read().ok()?.as_ref() {
            if *t == mtime {
                return Some(html.clone());
            }
        }
        let html = std::sync::Arc::new(std::fs::read_to_string(&path).ok()?);
        if let Ok(mut w) = self.template.write() {
            *w = Some((mtime, html.clone()));
        }
        Some(html)
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn clip(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n { s } else { format!("{}…", s.chars().take(n - 1).collect::<String>().trim_end()) }
}

/// Metadatos de una página.
#[derive(Default)]
struct Head {
    title: String,
    description: String,
    canonical: Option<String>,
    image: Option<String>,
    og_type: &'static str,
    noindex: bool,
    jsonld: Vec<Value>,
    status: u16,
    redirect: Option<String>,
}

impl Head {
    fn render(&self, store: &str) -> String {
        let title = if self.title.is_empty() { store.to_string() } else { format!("{} | {store}", self.title) };
        let mut h = format!("<title>{}</title>\n<meta name=\"description\" content=\"{}\" />\n", esc(&title), esc(&self.description));
        if self.noindex {
            h.push_str("<meta name=\"robots\" content=\"noindex, follow\" />\n");
        }
        if let Some(c) = &self.canonical {
            h.push_str(&format!("<link rel=\"canonical\" href=\"{}\" />\n<meta property=\"og:url\" content=\"{}\" />\n", esc(c), esc(c)));
        }
        h.push_str(&format!(
            "<meta property=\"og:site_name\" content=\"{}\" />\n<meta property=\"og:locale\" content=\"es_CL\" />\n<meta property=\"og:type\" content=\"{}\" />\n<meta property=\"og:title\" content=\"{}\" />\n<meta property=\"og:description\" content=\"{}\" />\n<meta name=\"twitter:card\" content=\"{}\" />\n",
            esc(store), if self.og_type.is_empty() { "website" } else { self.og_type }, esc(&title), esc(&self.description),
            if self.image.is_some() { "summary_large_image" } else { "summary" }));
        if let Some(img) = &self.image {
            h.push_str(&format!("<meta property=\"og:image\" content=\"{}\" />\n", esc(img)));
        }
        for j in &self.jsonld {
            // "</" escapado para que el JSON no pueda cerrar la etiqueta <script>
            h.push_str(&format!("<script type=\"application/ld+json\">{}</script>\n", j.to_string().replace("</", "<\\/")));
        }
        h
    }
}

fn abs(base: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") { url.to_string() } else { format!("{base}{url}") }
}

fn price_str(amount: i64, decimals: u32) -> String {
    if decimals == 0 { amount.to_string() } else { format!("{:.*}", decimals as usize, amount as f64 / 10f64.powi(decimals as i32)) }
}

fn breadcrumbs(base: &str, items: &[(String, String)]) -> Value {
    json!({
        "@context": "https://schema.org", "@type": "BreadcrumbList",
        "itemListElement": items.iter().enumerate().map(|(i, (name, path))| json!({
            "@type": "ListItem", "position": i + 1, "name": name, "item": format!("{base}{path}")
        })).collect::<Vec<_>>()
    })
}

fn category_trail(conn: &mut PgConnection, cat: &Categoria) -> ApiResult<Vec<(String, String)>> {
    let mut items = vec![("Inicio".to_string(), "/".to_string())];
    for c in category_services::ancestors(conn, cat)? {
        items.push((c.nombre.clone(), format!("/categoria/{}", c.slug)));
    }
    items.push((cat.nombre.clone(), format!("/categoria/{}", cat.slug)));
    Ok(items)
}

/// Calcula los metadatos según la ruta (consulta la BD cuando hace falta).
fn head_for(conn: &mut PgConnection, base: &str, path: &str, query: &str, tienda: &TiendaConfig, monedas: &MonedasConfig) -> ApiResult<Head> {
    let segs: Vec<&str> = path.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    let params: Vec<(String, String)> = url_pairs(query);
    let page = params.iter().find(|(k, _)| k == "page").and_then(|(_, v)| v.parse::<i64>().ok()).filter(|p| *p > 1);
    let filtered = params.iter().any(|(k, _)| k != "page");
    let with_page = |p: &str| match page { Some(n) => format!("{base}{p}?page={n}"), None => format!("{base}{p}") };
    let default_desc = if tienda.descripcion.is_empty() { format!("{}: tecnología y electrónica con despacho en Chile.", tienda.nombre) } else { tienda.descripcion.clone() };

    let head = match segs.as_slice() {
        [] => {
            let mut org = json!({ "@context": "https://schema.org", "@type": "Organization", "name": tienda.nombre, "url": format!("{base}/"), "logo": format!("{base}/favicon.svg") });
            let same: Vec<&String> = [&tienda.redes.instagram, &tienda.redes.facebook, &tienda.redes.tiktok, &tienda.redes.x].into_iter().filter(|u| !u.is_empty()).collect();
            if !same.is_empty() { org["sameAs"] = json!(same); }
            if !tienda.email.is_empty() || !tienda.telefono.is_empty() {
                org["contactPoint"] = json!({ "@type": "ContactPoint", "contactType": "customer service", "areaServed": "CL", "availableLanguage": "es",
                    "email": (!tienda.email.is_empty()).then_some(&tienda.email), "telephone": (!tienda.telefono.is_empty()).then_some(&tienda.telefono) });
            }
            Head {
                title: String::new(), description: clip(&default_desc, 160), canonical: Some(format!("{base}/")),
                jsonld: vec![org, json!({
                    "@context": "https://schema.org", "@type": "WebSite", "name": tienda.nombre, "url": format!("{base}/"), "inLanguage": "es-CL",
                    "potentialAction": { "@type": "SearchAction", "target": { "@type": "EntryPoint", "urlTemplate": format!("{base}/buscar?q={{search_term_string}}") }, "query-input": "required name=search_term_string" }
                })],
                ..Default::default()
            }
        }
        ["producto", key] => {
            let id = match product_services::resolve_key(conn, key) {
                Ok(id) => id,
                Err(_) => return Ok(not_found()),
            };
            let p = product_services::get_product(conn, id)?;
            if p.slug != *key {
                return Ok(Head { redirect: Some(format!("/producto/{}", p.slug)), status: 301, ..Default::default() });
            }
            if !p.estatus {
                return Ok(not_found());
            }
            let images: Vec<String> = product_images::table.filter(product_images::product_id.eq(id))
                .order((product_images::is_main.desc(), product_images::created_at.asc()))
                .select(product_images::image_url).limit(6).load::<String>(conn)?
                .into_iter().map(|u| abs(base, &u)).collect();
            let (avg, count): (Option<f64>, i64) = resenas::table.filter(resenas::producto_id.eq(id))
                .select((diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Double>>("AVG(calificacion)::float8"), diesel::dsl::count_star()))
                .first(conn)?;
            let base_m = monedas.lista.iter().find(|m| m.codigo == monedas.base);
            let dec = base_m.map_or(0, |m| m.decimales);
            let url = format!("{base}/producto/{}", p.slug);
            let mut trail = vec![("Inicio".to_string(), "/".to_string())];
            if let Some(cid) = p.categoria_id {
                if let Ok(cat) = category_services::get(conn, &cid.to_string()) {
                    trail = category_trail(conn, &cat)?;
                }
            }
            trail.push((p.nombre.clone(), format!("/producto/{}", p.slug)));
            let mut prod = json!({
                "@context": "https://schema.org", "@type": "Product", "name": p.nombre, "url": url, "image": images,
                "description": clip(p.descripcion.as_deref().unwrap_or(&p.nombre), 500),
                "offers": {
                    "@type": "Offer", "url": url, "priceCurrency": monedas.base, "price": price_str(p.precio, dec),
                    "availability": if p.stock > 0 { "https://schema.org/InStock" } else { "https://schema.org/OutOfStock" },
                    "itemCondition": "https://schema.org/NewCondition",
                    "seller": { "@type": "Organization", "name": tienda.nombre },
                },
            });
            if let Some(sku) = &p.sku { prod["sku"] = json!(sku); }
            if let Some(m) = &p.marca { prod["brand"] = json!({ "@type": "Brand", "name": m }); }
            if let (Some(a), true) = (avg, count > 0) {
                prod["aggregateRating"] = json!({ "@type": "AggregateRating", "ratingValue": format!("{a:.1}"), "reviewCount": count });
            }
            if let Some(g) = p.garantia_meses.filter(|g| *g > 0) {
                prod["additionalProperty"] = json!([{ "@type": "PropertyValue", "name": "Garantía", "value": format!("{g} meses") }]);
            }
            let precio = base_m.map(|m| format!("{}{}", m.simbolo, group(p.precio, dec))).unwrap_or_default();
            let desc = format!("{}{} a {precio}{}. {}", p.marca.as_ref().map(|m| format!("{m} ")).unwrap_or_default(), p.nombre,
                if p.stock > 0 { " con stock disponible" } else { " (sin stock)" }, p.descripcion.as_deref().unwrap_or(""));
            Head {
                title: format!("{}{}", p.nombre, p.marca.as_ref().map(|m| format!(" {m}")).filter(|m| !p.nombre.contains(m.trim())).unwrap_or_default()),
                description: clip(&desc, 160), canonical: Some(url), image: images.first().cloned(), og_type: "product",
                jsonld: vec![prod, breadcrumbs(base, &trail)], ..Default::default()
            }
        }
        ["categoria", slug] => {
            let cat: Option<Categoria> = categorias::table.filter(categorias::slug.eq(*slug)).filter(categorias::estatus.eq(true))
                .select(Categoria::as_select()).first(conn).optional()?;
            let Some(cat) = cat else { return Ok(not_found()) };
            let trail = category_trail(conn, &cat)?;
            let desc = cat.descripcion.clone().filter(|d| !d.trim().is_empty())
                .unwrap_or_else(|| format!("Compra {} en {}: precios en CLP, stock real y despacho en Chile.", cat.nombre.to_lowercase(), tienda.nombre));
            Head {
                title: format!("{}{}", cat.nombre, page.map(|n| format!(" - Página {n}")).unwrap_or_default()),
                description: clip(&desc, 160), canonical: Some(with_page(&format!("/categoria/{}", cat.slug))),
                noindex: filtered, jsonld: vec![breadcrumbs(base, &trail)], ..Default::default()
            }
        }
        ["marca", slug] => {
            let marca: Option<Option<String>> = productos::table
                .filter(productos::estatus.eq(true))
                .filter(diesel::dsl::sql::<diesel::sql_types::Bool>("jarstore_slugify(productos.marca) = ").bind::<diesel::sql_types::Text, _>(*slug))
                .select(productos::marca).first(conn).optional()?;
            let Some(Some(marca)) = marca else { return Ok(not_found()) };
            Head {
                title: format!("{marca}{}", page.map(|n| format!(" - Página {n}")).unwrap_or_default()),
                description: clip(&format!("Productos {marca} en {}: precios en CLP, stock real y despacho en Chile.", tienda.nombre), 160),
                canonical: Some(with_page(&format!("/marca/{slug}"))), noindex: filtered,
                jsonld: vec![breadcrumbs(base, &[("Inicio".into(), "/".into()), (marca.clone(), format!("/marca/{slug}"))])],
                ..Default::default()
            }
        }
        ["catalogo"] => Head {
            title: format!("Catálogo{}", page.map(|n| format!(" - Página {n}")).unwrap_or_default()),
            description: clip(&default_desc, 160), canonical: Some(with_page("/catalogo")), noindex: filtered, ..Default::default()
        },
        ["buscar"] => {
            let q = params.iter().find(|(k, _)| k == "q").map(|(_, v)| v.clone()).unwrap_or_default();
            Head { title: if q.is_empty() { "Buscar".into() } else { format!("Resultados para “{}”", clip(&q, 60)) },
                description: clip(&default_desc, 160), noindex: true, canonical: None, ..Default::default() }
        }
        ["p", slug] => {
            let row: Option<(String, String)> = paginas::table.filter(paginas::slug.eq(*slug)).filter(paginas::visible.eq(true))
                .select((paginas::titulo, paginas::contenido)).first(conn).optional()?;
            let Some((titulo, contenido)) = row else { return Ok(not_found()) };
            Head { title: titulo, description: clip(&contenido.replace('#', ""), 160), canonical: Some(format!("{base}/p/{slug}")), ..Default::default() }
        }
        // Áreas privadas o de proceso: no indexar
        [first, ..] if ["cuenta", "carrito", "checkout", "pedido", "admin", "vendedor", "login", "registro", "recuperar", "restablecer", "newsletter", "ordenes", "wishlist"].contains(first) => Head {
            title: String::new(), description: clip(&default_desc, 160), noindex: true, ..Default::default()
        },
        _ => not_found(),
    };
    Ok(head)
}

fn not_found() -> Head {
    Head { title: "Página no encontrada".into(), noindex: true, status: 404, ..Default::default() }
}

fn group(amount: i64, decimals: u32) -> String {
    let int = amount / 10i64.pow(decimals);
    let digits = int.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) { out.push('.'); }
        out.push(c);
    }
    out
}

fn url_pairs(query: &str) -> Vec<(String, String)> {
    query.split('&').filter(|s| !s.is_empty()).map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (decode(k), decode(v))
    }).collect()
}

fn decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) { out.push(b); i += 3; continue; }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// GET de cualquier ruta del frontend: archivo estático o index.html con metadatos.
pub async fn frontend(req: HttpRequest, site: web::Data<Site>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let path = req.path().to_string();
    if path.starts_with("/api/") || path == "/api" {
        return Err(ApiError::not_found("Ruta no encontrada"));
    }
    let (Some(dist), Some(template)) = (&site.dist, site.template()) else {
        return Err(ApiError::not_found("Ruta no encontrada"));
    };
    // Archivos estáticos (assets con hash => caché de 1 año)
    if path != "/" && !path.contains("..") {
        let file = dist.join(path.trim_start_matches('/'));
        if file.is_file() && file.starts_with(dist) {
            let named = actix_files::NamedFile::open_async(&file).await.map_err(|e| ApiError::internal(e.to_string()))?;
            let mut res = named.into_response(&req);
            let cache = if path.starts_with("/assets/") { "public, max-age=31536000, immutable" } else { "public, max-age=3600" };
            res.headers_mut().insert(header::CACHE_CONTROL, header::HeaderValue::from_static(cache));
            return Ok(res);
        }
        if path.rsplit('/').next().is_some_and(|last| last.contains('.')) {
            return Ok(HttpResponse::NotFound().finish()); // p. ej. /logo.png inexistente
        }
    }
    let base = site.public_url.clone();
    let query = req.query_string().to_string();
    let (head, store) = db::run(&pool, move |conn| {
        let tienda = tienda_services::tienda(conn)?;
        let monedas = tienda_services::monedas(conn)?;
        let head = head_for(conn, &base, &path, &query, &tienda, &monedas)?;
        Ok((head, tienda.nombre))
    }).await?;
    if let Some(to) = head.redirect {
        let q = req.query_string();
        let loc = if q.is_empty() { to } else { format!("{to}?{q}") };
        return Ok(HttpResponse::MovedPermanently().insert_header((header::LOCATION, loc)).finish());
    }
    let injected = format!("{}{}", head.render(&store), site.analytics);
    let html = match (template.find("<!--seo-->"), template.find("<!--/seo-->")) {
        (Some(a), Some(b)) => format!("{}{}{}", &template[..a], injected, &template[b + "<!--/seo-->".len()..]),
        _ => template.replacen("</head>", &format!("{injected}</head>"), 1),
    };
    let status = actix_web::http::StatusCode::from_u16(if head.status == 0 { 200 } else { head.status }).unwrap_or(actix_web::http::StatusCode::OK);
    Ok(HttpResponse::build(status)
        .content_type("text/html; charset=utf-8")
        .insert_header((header::CACHE_CONTROL, "no-cache"))
        .body(html))
}

pub async fn robots(site: web::Data<Site>) -> HttpResponse {
    let body = format!(
        "User-agent: *\nDisallow: /cuenta\nDisallow: /carrito\nDisallow: /checkout\nDisallow: /pedido\nDisallow: /admin\nDisallow: /vendedor\nDisallow: /buscar\nDisallow: /api/\nAllow: /api/uploads\n\nSitemap: {}/sitemap.xml\n",
        site.public_url);
    HttpResponse::Ok().content_type("text/plain; charset=utf-8").insert_header((header::CACHE_CONTROL, "public, max-age=86400")).body(body)
}

pub async fn sitemap(site: web::Data<Site>, pool: web::Data<DbPool>) -> ApiResult<HttpResponse> {
    let base = site.public_url.clone();
    let xml = db::run(&pool, move |conn| {
        let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" xmlns:image=\"http://www.google.com/schemas/sitemap-image/1.1\">\n");
        let mut url = |loc: String, lastmod: Option<chrono::NaiveDateTime>, img: Option<String>| {
            x.push_str(&format!("  <url><loc>{}</loc>", esc(&loc)));
            if let Some(d) = lastmod { x.push_str(&format!("<lastmod>{}</lastmod>", d.format("%Y-%m-%d"))); }
            if let Some(i) = img { x.push_str(&format!("<image:image><image:loc>{}</image:loc></image:image>", esc(&i))); }
            x.push_str("</url>\n");
        };
        url(format!("{base}/"), None, None);
        url(format!("{base}/catalogo"), None, None);
        for (slug, upd) in categorias::table.filter(categorias::estatus.eq(true)).select((categorias::slug, categorias::updated_at)).load::<(String, chrono::NaiveDateTime)>(conn)? {
            url(format!("{base}/categoria/{slug}"), Some(upd), None);
        }
        #[derive(QueryableByName)]
        struct M { #[diesel(sql_type = diesel::sql_types::Text)] s: String }
        for m in diesel::sql_query("SELECT DISTINCT jarstore_slugify(marca) AS s FROM productos WHERE estatus AND marca IS NOT NULL AND marca <> ''").load::<M>(conn)? {
            url(format!("{base}/marca/{}", m.s), None, None);
        }
        let prods: Vec<(uuid::Uuid, String, chrono::NaiveDateTime)> = productos::table.filter(productos::estatus.eq(true))
            .order(productos::updated_at.desc()).limit(45_000).select((productos::id, productos::slug, productos::updated_at)).load(conn)?;
        let ids: Vec<uuid::Uuid> = prods.iter().map(|p| p.0).collect();
        let imgs = product_services::main_images_for(conn, &ids)?;
        for (id, slug, upd) in prods {
            url(format!("{base}/producto/{slug}"), Some(upd), imgs.get(&id).map(|u| abs(&base, u)));
        }
        for (slug, upd) in paginas::table.filter(paginas::visible.eq(true)).select((paginas::slug, paginas::updated_at)).load::<(String, chrono::NaiveDateTime)>(conn)? {
            url(format!("{base}/p/{slug}"), Some(upd), None);
        }
        x.push_str("</urlset>\n");
        Ok(x)
    }).await?;
    Ok(HttpResponse::Ok().content_type("application/xml; charset=utf-8").insert_header((header::CACHE_CONTROL, "public, max-age=3600")).body(xml))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapa_y_recorta() {
        assert_eq!(esc("a<b>\"c\"&"), "a&lt;b&gt;&quot;c&quot;&amp;");
        assert_eq!(clip("hola   mundo", 20), "hola mundo");
        assert_eq!(clip("abcdefghij", 5), "abcd…");
        assert_eq!(group(1299990, 0), "1.299.990");
        assert_eq!(price_str(12990, 0), "12990");
        assert_eq!(decode("notebook+pro%20%C3%B1"), "notebook pro ñ");
        let h = Head { jsonld: vec![json!({"x": "</script><script>alert(1)"})], ..Default::default() };
        assert!(!h.render("S").contains("</script><script>"));
    }
}
