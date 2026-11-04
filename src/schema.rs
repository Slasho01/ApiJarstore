// Esquema de Diesel. Refleja ambas migraciones en /migrations.
// Si instalas diesel_cli puedes regenerarlo con: diesel print-schema > src/schema.rs

diesel::table! {
    carrito (id) {
        id -> Uuid,
        user_id -> Uuid,
        producto_id -> Uuid,
        variante_id -> Nullable<Uuid>,
        cantidad -> Int4,
        created_at -> Timestamp,
    }
}

diesel::table! {
    avisos_stock (id) {
        id -> Uuid,
        producto_id -> Uuid,
        variante_id -> Nullable<Uuid>,
        #[max_length = 255]
        email -> Varchar,
        user_id -> Nullable<Uuid>,
        created_at -> Timestamp,
        notificado_at -> Nullable<Timestamp>,
    }
}

diesel::table! {
    carrito_avisos (user_id) {
        user_id -> Uuid,
        enviado_at -> Timestamp,
    }
}

diesel::table! {
    categorias (id) {
        id -> Uuid,
        #[max_length = 255]
        nombre -> Varchar,
        descripcion -> Nullable<Text>,
        estatus -> Bool,
        created_at -> Timestamp,
        updated_at -> Timestamp,
        parent_id -> Nullable<Uuid>,
        #[max_length = 120]
        slug -> Varchar,
        orden -> Int4,
        #[max_length = 40]
        icono -> Nullable<Varchar>,
    }
}

diesel::table! {
    cupones (id) {
        id -> Uuid,
        #[max_length = 100]
        codigo -> Varchar,
        descuento_porcentaje -> Nullable<Int4>,
        descuento_fijo -> Nullable<Int8>,
        fecha_expiracion -> Nullable<Timestamp>,
        uso_maximo -> Nullable<Int4>,
        uso_actual -> Int4,
        activo -> Bool,
        created_at -> Timestamp,
    }
}

diesel::table! {
    detalle_orden (id) {
        id -> Uuid,
        orden_id -> Nullable<Uuid>,
        producto_id -> Nullable<Uuid>,
        variante_id -> Nullable<Uuid>,
        cantidad -> Int4,
        precio_unitario -> Int8,
        #[max_length = 255]
        nombre_producto -> Varchar,
        #[max_length = 255]
        nombre_variante -> Nullable<Varchar>,
    }
}

diesel::table! {
    direcciones (id) {
        id -> Uuid,
        user_id -> Uuid,
        #[max_length = 100]
        titulo -> Nullable<Varchar>,
        direccion -> Text,
        #[max_length = 100]
        ciudad -> Nullable<Varchar>,
        #[max_length = 100]
        estado -> Nullable<Varchar>,
        #[max_length = 20]
        codigo_postal -> Nullable<Varchar>,
        #[max_length = 100]
        pais -> Nullable<Varchar>,
        #[max_length = 50]
        telefono -> Nullable<Varchar>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    envios (id) {
        id -> Uuid,
        orden_id -> Nullable<Uuid>,
        #[max_length = 255]
        empresa_envio -> Nullable<Varchar>,
        #[max_length = 255]
        codigo_seguimiento -> Nullable<Varchar>,
        #[max_length = 50]
        estatus -> Nullable<Varchar>,
        fecha_envio -> Nullable<Timestamp>,
        fecha_entrega -> Nullable<Timestamp>,
    }
}

diesel::table! {
    ordenes (id) {
        id -> Uuid,
        user_id -> Nullable<Uuid>,
        total -> Int8,
        #[max_length = 50]
        estatus -> Varchar,
        #[max_length = 50]
        metodo_pago -> Nullable<Varchar>,
        direccion_envio -> Nullable<Text>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
        subtotal -> Int8,
        descuento -> Int8,
        cupon_id -> Nullable<Uuid>,
        costo_envio -> Int8,
        #[max_length = 100]
        metodo_envio -> Nullable<Varchar>,
        #[max_length = 2]
        pais -> Nullable<Varchar>,
        #[max_length = 3]
        moneda -> Varchar,
        tasa_cambio -> Float8,
        #[max_length = 255]
        nombre_comprador -> Nullable<Varchar>,
        #[max_length = 255]
        email_comprador -> Nullable<Varchar>,
        #[max_length = 50]
        telefono_comprador -> Nullable<Varchar>,
        #[max_length = 30]
        pago_proveedor -> Nullable<Varchar>,
        puntos_usados -> Int4,
        descuento_puntos -> Int8,
        puntos_ganados -> Int4,
        #[max_length = 10]
        tipo_documento -> Nullable<Varchar>,
        #[max_length = 20]
        factura_rut -> Nullable<Varchar>,
        #[max_length = 255]
        factura_razon_social -> Nullable<Varchar>,
        #[max_length = 255]
        factura_giro -> Nullable<Varchar>,
        factura_direccion -> Nullable<Text>,
        #[max_length = 500]
        documento_url -> Nullable<Varchar>,
        documento_subido_at -> Nullable<Timestamp>,
        #[max_length = 20]
        comuna_destino -> Nullable<Varchar>,
    }
}

diesel::table! {
    puntos_movimientos (id) {
        id -> Uuid,
        user_id -> Uuid,
        orden_id -> Nullable<Uuid>,
        #[max_length = 20]
        tipo -> Varchar,
        puntos -> Int4,
        #[max_length = 255]
        descripcion -> Nullable<Varchar>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    configuracion (clave) {
        #[max_length = 50]
        clave -> Varchar,
        valor -> Jsonb,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    paginas (id) {
        id -> Uuid,
        #[max_length = 100]
        slug -> Varchar,
        #[max_length = 255]
        titulo -> Varchar,
        contenido -> Text,
        visible -> Bool,
        en_footer -> Bool,
        orden -> Int4,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    password_resets (id) {
        id -> Uuid,
        user_id -> Uuid,
        #[max_length = 64]
        token_hash -> Varchar,
        expires_at -> Timestamp,
        used_at -> Nullable<Timestamp>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    pagos (id) {
        id -> Uuid,
        orden_id -> Nullable<Uuid>,
        monto -> Int8,
        #[max_length = 50]
        metodo -> Nullable<Varchar>,
        #[max_length = 50]
        estatus -> Nullable<Varchar>,
        #[max_length = 255]
        referencia_externa -> Nullable<Varchar>,
        created_at -> Timestamp,
        #[max_length = 30]
        proveedor -> Nullable<Varchar>,
    }
}

diesel::table! {
    perfil (id) {
        id -> Uuid,
        user_id -> Nullable<Uuid>,
        #[max_length = 255]
        nombre -> Varchar,
        #[max_length = 255]
        apellido -> Varchar,
        #[max_length = 255]
        telefono -> Nullable<Varchar>,
        direccion -> Nullable<Text>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    product_images (id) {
        id -> Uuid,
        product_id -> Uuid,
        #[max_length = 500]
        image_url -> Varchar,
        #[max_length = 255]
        alt_text -> Nullable<Varchar>,
        is_main -> Bool,
        created_at -> Timestamp,
        variantes -> Jsonb,
    }
}

diesel::table! {
    product_variant_images (id) {
        id -> Uuid,
        variant_id -> Uuid,
        #[max_length = 500]
        image_url -> Varchar,
        #[max_length = 255]
        alt_text -> Nullable<Varchar>,
        is_main -> Bool,
        created_at -> Timestamp,
        variantes -> Jsonb,
    }
}

diesel::table! {
    product_variants (id) {
        id -> Uuid,
        producto_id -> Uuid,
        #[max_length = 255]
        nombre -> Varchar,
        #[max_length = 255]
        valor -> Varchar,
        stock -> Int4,
        precio_extra -> Int8,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    productos (id) {
        id -> Uuid,
        #[max_length = 255]
        nombre -> Varchar,
        descripcion -> Nullable<Text>,
        precio -> Int8,
        stock -> Int4,
        #[max_length = 255]
        sku -> Nullable<Varchar>,
        estatus -> Bool,
        categoria_id -> Nullable<Uuid>,
        user_id -> Nullable<Uuid>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
        #[max_length = 100]
        marca -> Nullable<Varchar>,
        precio_anterior -> Nullable<Int8>,
        destacado -> Bool,
        especificaciones -> Jsonb,
        compatibilidad -> Nullable<Text>,
        garantia_meses -> Nullable<Int4>,
        #[max_length = 160]
        slug -> Varchar,
        peso_gramos -> Nullable<Int4>,
        largo_cm -> Nullable<Int4>,
        ancho_cm -> Nullable<Int4>,
        alto_cm -> Nullable<Int4>,
        melihub_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    producto_slugs_antiguos (slug) {
        #[max_length = 160]
        slug -> Varchar,
        producto_id -> Uuid,
        created_at -> Timestamp,
    }
}

diesel::table! {
    producto_complementos (producto_id, complemento_id) {
        producto_id -> Uuid,
        complemento_id -> Uuid,
        orden -> Int4,
    }
}

diesel::table! {
    uploads (id) {
        id -> Uuid,
        user_id -> Nullable<Uuid>,
        #[max_length = 20]
        proveedor -> Varchar,
        #[max_length = 300]
        clave -> Varchar,
        #[max_length = 500]
        url -> Varchar,
        variantes -> Jsonb,
        ancho -> Nullable<Int4>,
        alto -> Nullable<Int4>,
        bytes -> Nullable<Int8>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    banners (id) {
        id -> Uuid,
        #[max_length = 20]
        ubicacion -> Varchar,
        #[max_length = 160]
        titulo -> Varchar,
        #[max_length = 300]
        subtitulo -> Nullable<Varchar>,
        #[max_length = 60]
        texto_boton -> Nullable<Varchar>,
        #[max_length = 500]
        enlace -> Nullable<Varchar>,
        #[max_length = 500]
        imagen_url -> Nullable<Varchar>,
        imagen_variantes -> Jsonb,
        #[max_length = 500]
        imagen_movil_url -> Nullable<Varchar>,
        #[max_length = 10]
        tema -> Varchar,
        orden -> Int4,
        activo -> Bool,
        desde -> Nullable<Timestamp>,
        hasta -> Nullable<Timestamp>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    newsletter (id) {
        id -> Uuid,
        #[max_length = 255]
        email -> Varchar,
        #[max_length = 64]
        token -> Varchar,
        confirmado_at -> Nullable<Timestamp>,
        baja_at -> Nullable<Timestamp>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    resenas (id) {
        id -> Uuid,
        user_id -> Nullable<Uuid>,
        producto_id -> Nullable<Uuid>,
        calificacion -> Int4,
        comentario -> Nullable<Text>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    users (id) {
        id -> Uuid,
        #[max_length = 255]
        username -> Varchar,
        #[max_length = 255]
        email -> Varchar,
        #[max_length = 255]
        password_hash -> Varchar,
        #[max_length = 20]
        role -> Varchar,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    wishlist (id) {
        id -> Uuid,
        user_id -> Uuid,
        producto_id -> Uuid,
        created_at -> Timestamp,
    }
}

diesel::joinable!(carrito -> product_variants (variante_id));
diesel::joinable!(carrito -> productos (producto_id));
diesel::joinable!(carrito -> users (user_id));
diesel::joinable!(detalle_orden -> ordenes (orden_id));
diesel::joinable!(detalle_orden -> product_variants (variante_id));
diesel::joinable!(detalle_orden -> productos (producto_id));
diesel::joinable!(direcciones -> users (user_id));
diesel::joinable!(envios -> ordenes (orden_id));
diesel::joinable!(ordenes -> cupones (cupon_id));
diesel::joinable!(ordenes -> users (user_id));
diesel::joinable!(pagos -> ordenes (orden_id));
diesel::joinable!(password_resets -> users (user_id));
diesel::joinable!(perfil -> users (user_id));
diesel::joinable!(puntos_movimientos -> users (user_id));
diesel::joinable!(puntos_movimientos -> ordenes (orden_id));
diesel::joinable!(product_images -> productos (product_id));
diesel::joinable!(product_variant_images -> product_variants (variant_id));
diesel::joinable!(product_variants -> productos (producto_id));
diesel::joinable!(productos -> categorias (categoria_id));
diesel::joinable!(producto_slugs_antiguos -> productos (producto_id));
diesel::joinable!(uploads -> users (user_id));
diesel::joinable!(productos -> users (user_id));
diesel::joinable!(resenas -> productos (producto_id));
diesel::joinable!(resenas -> users (user_id));
diesel::joinable!(wishlist -> productos (producto_id));
diesel::joinable!(wishlist -> users (user_id));
diesel::joinable!(avisos_stock -> productos (producto_id));
diesel::joinable!(avisos_stock -> product_variants (variante_id));
diesel::joinable!(avisos_stock -> users (user_id));
diesel::joinable!(carrito_avisos -> users (user_id));

diesel::allow_tables_to_appear_in_same_query!(
    avisos_stock,
    banners,
    carrito,
    carrito_avisos,
    categorias,
    configuracion,
    cupones,
    detalle_orden,
    direcciones,
    envios,
    newsletter,
    ordenes,
    pagos,
    paginas,
    password_resets,
    perfil,
    product_images,
    product_variant_images,
    product_variants,
    puntos_movimientos,
    producto_complementos,
    producto_slugs_antiguos,
    productos,
    resenas,
    uploads,
    users,
    wishlist,
);
