use crate::errors::{ApiError, ApiResult};
use chrono::{NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

pub fn now() -> NaiveDateTime {
    Utc::now().naive_utc()
}

#[derive(Deserialize, Debug, Clone, Copy, Default)]
pub struct Pagination {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

impl Pagination {
    pub fn page(&self) -> i64 {
        self.page.unwrap_or(1).max(1)
    }
    pub fn per_page(&self) -> i64 {
        self.per_page.unwrap_or(20).clamp(1, 100)
    }
    pub fn offset(&self) -> i64 {
        (self.page() - 1) * self.per_page()
    }
}

#[derive(Serialize, Debug)]
pub struct Paginated<T> {
    pub items: Vec<T>,
    pub page: i64,
    pub per_page: i64,
    pub total: i64,
    pub total_pages: i64,
}

impl<T> Paginated<T> {
    pub fn new(items: Vec<T>, p: &Pagination, total: i64) -> Self {
        let per_page = p.per_page();
        Paginated {
            items,
            page: p.page(),
            per_page,
            total,
            total_pages: (total + per_page - 1) / per_page,
        }
    }
}

/// Texto obligatorio: se recorta y se valida longitud.
pub fn required_text(field: &str, value: &str, max: usize) -> ApiResult<String> {
    let v = value.trim();
    if v.is_empty() {
        return Err(ApiError::bad_request(format!("El campo '{field}' es obligatorio")));
    }
    if v.chars().count() > max {
        return Err(ApiError::bad_request(format!("El campo '{field}' admite máximo {max} caracteres")));
    }
    Ok(v.to_string())
}

/// Texto opcional: cadenas vacías se tratan como `None`.
pub fn optional_text(field: &str, value: Option<String>, max: usize) -> ApiResult<Option<String>> {
    match value {
        None => Ok(None),
        Some(v) if v.trim().is_empty() => Ok(None),
        Some(v) => required_text(field, &v, max).map(Some),
    }
}

pub fn non_negative<T: PartialOrd + Default + Copy>(field: &str, value: T) -> ApiResult<T> {
    if value < T::default() {
        return Err(ApiError::bad_request(format!("El campo '{field}' no puede ser negativo")));
    }
    Ok(value)
}

pub fn positive_qty(value: i32) -> ApiResult<i32> {
    if !(1..=10_000).contains(&value) {
        return Err(ApiError::bad_request("La cantidad debe estar entre 1 y 10000"));
    }
    Ok(value)
}

pub fn valid_url(field: &str, value: &str) -> ApiResult<String> {
    let v = required_text(field, value, 500)?;
    if !(v.starts_with("http://") || v.starts_with("https://") || v.starts_with('/')) {
        return Err(ApiError::bad_request(format!("El campo '{field}' debe ser una URL http(s) o una ruta absoluta")));
    }
    Ok(v)
}

/// Para campos que se pueden "borrar": ausente => None, null => Some(None), valor => Some(Some(v)).
/// Usar junto con `#[serde(default, deserialize_with = "double_option")]`.
pub fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// Convierte un texto en slug para URLs: "Audífonos Pro 2" -> "audifonos-pro-2".
/// Debe coincidir con la función SQL `jarstore_slugify`.
pub fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut dash = false;
    for c in text.chars() {
        let c = match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'Á' | 'À' | 'Ä' | 'Â' | 'Ã' => 'a',
            'é' | 'è' | 'ë' | 'ê' | 'É' | 'È' | 'Ë' | 'Ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' | 'Í' | 'Ì' | 'Ï' | 'Î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' | 'Ó' | 'Ò' | 'Ö' | 'Ô' | 'Õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' | 'Ú' | 'Ù' | 'Ü' | 'Û' => 'u',
            'ñ' | 'Ñ' => 'n',
            'ç' | 'Ç' => 'c',
            c => c.to_ascii_lowercase(),
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let s = out.trim_end_matches('-').to_string();
    if s.is_empty() { "item".into() } else { s }
}

pub fn valid_slug(field: &str, slug: &str, max: usize) -> ApiResult<String> {
    let s = slug.trim().to_lowercase();
    let ok = !s.is_empty() && s.len() <= max
        && s.split('-').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    if !ok {
        return Err(ApiError::bad_request(format!("'{field}' sólo admite minúsculas, números y guiones (p. ej. 'notebook-pro-14')")));
    }
    Ok(s)
}

/// Busca un slug libre añadiendo -2, -3… (`exists` dice si ya está usado).
pub fn unique_slug(base: &str, max: usize, mut exists: impl FnMut(&str) -> ApiResult<bool>) -> ApiResult<String> {
    let base: String = base.chars().take(max - 4).collect::<String>().trim_end_matches('-').to_string();
    if !exists(&base)? {
        return Ok(base);
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !exists(&candidate)? {
            return Ok(candidate);
        }
    }
    Err(ApiError::conflict("No se pudo generar un slug único"))
}

#[cfg(test)]
mod tests {
    use super::slugify;
    #[test]
    fn slugs() {
        assert_eq!(slugify("Audífonos Pro 2"), "audifonos-pro-2");
        assert_eq!(slugify("  Computación & Gamer!! "), "computacion-gamer");
        assert_eq!(slugify("Notebook 14\" ÑANDÚ"), "notebook-14-nandu");
        assert_eq!(slugify("¿?"), "item");
    }
}

/// Valida y normaliza un RUT chileno (dígito verificador módulo 11).
/// Acepta "12.345.678-5", "12345678-5" o "123456785"; devuelve "12345678-5".
pub fn validate_rut(rut: &str) -> ApiResult<String> {
    let clean: String = rut.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase();
    if clean.len() < 2 || clean.len() > 9 {
        return Err(ApiError::bad_request("RUT inválido"));
    }
    let (cuerpo, dv) = clean.split_at(clean.len() - 1);
    if !cuerpo.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::bad_request("RUT inválido"));
    }
    let mut suma = 0u32;
    let mut factor = 2u32;
    for c in cuerpo.chars().rev() {
        suma += c.to_digit(10).unwrap() * factor;
        factor = if factor == 7 { 2 } else { factor + 1 };
    }
    let resto = 11 - (suma % 11);
    let esperado = match resto { 11 => '0', 10 => 'K', n => char::from_digit(n, 10).unwrap() };
    if !dv.starts_with(esperado) {
        return Err(ApiError::bad_request("El RUT no es válido (dígito verificador incorrecto)"));
    }
    Ok(format!("{cuerpo}-{esperado}"))
}

#[cfg(test)]
mod rut_tests {
    use super::validate_rut;
    #[test]
    fn rut() {
        assert_eq!(validate_rut("12.345.678-5").unwrap(), "12345678-5");
        assert_eq!(validate_rut("11111111-1").unwrap(), "11111111-1");
        assert!(validate_rut("12345678-9").is_err()); // DV incorrecto
        assert!(validate_rut("no-rut").is_err());
    }
}
