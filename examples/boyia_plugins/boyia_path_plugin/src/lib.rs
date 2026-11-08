mod path_plugin;

boyia_plugin_sdk::boyia_plugin_export! {
    name: "boyia_path_plugin",
    registrars: [path_plugin::builtin_path_plugin_class]
}
