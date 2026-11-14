mod scheduler;
mod timer_plugin;

boyia_plugin_sdk::boyia_plugin_export! {
    name: "boyia_timer_plugin",
    registrars: [timer_plugin::builtin_timer_plugin_class]
}
