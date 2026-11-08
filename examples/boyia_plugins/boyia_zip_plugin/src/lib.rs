mod zip_plugin;

boyia_plugin_sdk::boyia_plugin_export! {
    name: "boyia_zip_plugin",
    registrars: [zip_plugin::builtin_zip_plugin_class]
}
