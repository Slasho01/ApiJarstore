use crate::errors::{ApiError, ApiResult, OrNotFound};
use crate::models::carrito::{CarritoItem, NewCarritoItem};
use crate::models::products::{Producto, Variante};
use crate::schema::{carrito, product_variants, productos};
use crate::services::common::positive_qty;
use crate::services::product_services::{get_product, get_variant, main_images_for};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct AddToCartRequest {
    pub producto_id: Uuid,
    pub variante_id: Option<Uuid>,
    #[serde(default = "one")]
    pub cantidad: i32,
}

fn one() -> i32 {
    1
}

#[derive(Deserialize)]
pub struct UpdateQuantityRequest {
    pub cantidad: i32,
}

#[derive(Serialize)]
pub struct CartLine {
    pub id: Uuid,
    pub producto_id: Uuid,
    pub nombre: String,
    pub marca: Option<String>,
    pub imagen: Option<String>,
    pub variante_id: Option<Uuid>,
    pub variante: Option<String>,
    pub precio_unitario: i64,
    pub cantidad: i32,
    pub subtotal: i64,
    pub stock_disponible: i32,
    /// false si el producto se desactivó o ya no hay stock suficiente
    pub disponible: bool,
}

#[derive(Serialize)]
pub struct CartView {
    pub items: Vec<CartLine>,
    pub total_items: i64,
    pub total: i64,
}

/// Valida producto/variante y devuelve (stock disponible, precio unitario).
pub fn check_purchasable(conn: &mut PgConnection, producto_id: Uuid, variante_id: Option<Uuid>) -> ApiResult<(Producto, Option<Variante>)> {
    let producto = get_product(conn, producto_id)?;
    if !producto.estatus {
        return Err(ApiError::bad_request(format!("El producto '{}' no está disponible", producto.nombre)));
    }
    let variante = match variante_id {
        Some(vid) => {
            let v = get_variant(conn, vid)?;
            if v.producto_id != producto_id {
                return Err(ApiError::bad_request("La variante no pertenece a ese producto"));
            }
            Some(v)
        }
        None => None,
    };
    Ok((producto, variante))
}

pub fn view(conn: &mut PgConnection, user_id: Uuid) -> ApiResult<CartView> {
    let rows: Vec<(CarritoItem, Producto, Option<Variante>)> = carrito::table
        .inner_join(productos::table)
        .left_join(product_variants::table)
        .filter(carrito::user_id.eq(user_id))
        .order(carrito::created_at.asc())
        .select((CarritoItem::as_select(), Producto::as_select(), Option::<Variante>::as_select()))
        .load(conn)?;

    let ids: Vec<Uuid> = rows.iter().map(|(_, p, _)| p.id).collect();
    let images = main_images_for(conn, &ids)?;
    let items: Vec<CartLine> = rows
        .into_iter()
        .map(|(item, p, v)| {
            let precio_unitario = p.precio + v.as_ref().map_or(0, |v| v.precio_extra);
            let stock = v.as_ref().map_or(p.stock, |v| v.stock);
            CartLine {
                id: item.id,
                producto_id: p.id,
                imagen: images.get(&p.id).cloned(),
                marca: p.marca,
                nombre: p.nombre,
                variante_id: item.variante_id,
                variante: v.map(|v| format!("{}: {}", v.nombre, v.valor)),
                precio_unitario,
                cantidad: item.cantidad,
                subtotal: precio_unitario * item.cantidad as i64,
                stock_disponible: stock,
                disponible: p.estatus && stock >= item.cantidad,
            }
        })
        .collect();
    Ok(CartView {
        total_items: items.iter().map(|i| i.cantidad as i64).sum(),
        total: items.iter().map(|i| i.subtotal).sum(),
        items,
    })
}

pub fn add(conn: &mut PgConnection, user_id: Uuid, req: AddToCartRequest) -> ApiResult<CartView> {
    positive_qty(req.cantidad)?;
    conn.transaction(|conn| {
        let (producto, variante) = check_purchasable(conn, req.producto_id, req.variante_id)?;
        let stock = variante.as_ref().map_or(producto.stock, |v| v.stock);

        let existing: Option<CarritoItem> = carrito::table
            .filter(carrito::user_id.eq(user_id))
            .filter(carrito::producto_id.eq(req.producto_id))
            .filter(carrito::variante_id.is_not_distinct_from(req.variante_id))
            .select(CarritoItem::as_select())
            .for_update()
            .first(conn)
            .optional()?;

        let nueva_cantidad = existing.as_ref().map_or(0, |e| e.cantidad) + req.cantidad;
        if nueva_cantidad > stock {
            return Err(ApiError::bad_request(format!("Stock insuficiente: disponibles {stock}")));
        }
        match existing {
            Some(item) => {
                diesel::update(carrito::table.find(item.id))
                    .set(carrito::cantidad.eq(nueva_cantidad))
                    .execute(conn)?;
            }
            None => {
                diesel::insert_into(carrito::table)
                    .values(NewCarritoItem {
                        user_id,
                        producto_id: req.producto_id,
                        variante_id: req.variante_id,
                        cantidad: nueva_cantidad,
                    })
                    .execute(conn)?;
            }
        }
        view(conn, user_id)
    })
}

/// Cambia la cantidad de una línea. Cantidad <= 0 elimina la línea.
pub fn update_quantity(conn: &mut PgConnection, user_id: Uuid, item_id: Uuid, cantidad: i32) -> ApiResult<CartView> {
    let item: CarritoItem = carrito::table
        .filter(carrito::id.eq(item_id).and(carrito::user_id.eq(user_id)))
        .select(CarritoItem::as_select())
        .first(conn)
        .or_not_found("Item del carrito")?;
    if cantidad <= 0 {
        diesel::delete(carrito::table.find(item.id)).execute(conn)?;
    } else {
        positive_qty(cantidad)?;
        let (producto, variante) = check_purchasable(conn, item.producto_id, item.variante_id)?;
        let stock = variante.as_ref().map_or(producto.stock, |v| v.stock);
        if cantidad > stock {
            return Err(ApiError::bad_request(format!("Stock insuficiente: disponibles {stock}")));
        }
        diesel::update(carrito::table.find(item.id))
            .set(carrito::cantidad.eq(cantidad))
            .execute(conn)?;
    }
    view(conn, user_id)
}

pub fn remove(conn: &mut PgConnection, user_id: Uuid, item_id: Uuid) -> ApiResult<()> {
    let n = diesel::delete(carrito::table.filter(carrito::id.eq(item_id).and(carrito::user_id.eq(user_id)))).execute(conn)?;
    if n == 0 {
        return Err(ApiError::not_found("Item del carrito no encontrado"));
    }
    Ok(())
}

pub fn clear(conn: &mut PgConnection, user_id: Uuid) -> ApiResult<usize> {
    Ok(diesel::delete(carrito::table.filter(carrito::user_id.eq(user_id))).execute(conn)?)
}
