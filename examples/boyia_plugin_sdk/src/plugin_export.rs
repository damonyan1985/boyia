//! Export registrar table for dylib plugins (pure Rust function pointers, no C structs).

/// Registers plugin builtins with the host loader.
///
/// ```ignore
/// boyia_plugin_sdk::boyia_plugin_export! {
///     name: "boyia_zip_plugin",
///     registrars: [zip::builtin_zip_class]
/// }
/// ```
#[macro_export]
macro_rules! boyia_plugin_export {
    (
        name: $name:literal,
        registrars: [ $( $registrar:path ),* ]
    ) => {
        static BOYIA_PLUGIN_REGISTRARS: &[$crate::registrar::BuiltinRegistrar] =
            &[ $( $registrar ),* ];

        #[no_mangle]
        pub extern "C" fn boyia_plugin_registrars_ptr() -> *const $crate::registrar::BuiltinRegistrar {
            BOYIA_PLUGIN_REGISTRARS.as_ptr()
        }

        #[no_mangle]
        pub extern "C" fn boyia_plugin_registrars_len() -> usize {
            BOYIA_PLUGIN_REGISTRARS.len()
        }

        #[no_mangle]
        pub extern "C" fn boyia_plugin_name() -> *const std::os::raw::c_char {
            concat!($name, "\0").as_ptr() as *const std::os::raw::c_char
        }
    };
}
