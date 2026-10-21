use cxx_qt_build::CxxQtBuilder;
use cxx_qt_build::PluginType;
use cxx_qt_build::QmlModule;

fn main() {
    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("Kotodama")
            .plugin_type(PluginType::Dynamic)
            .qml_file("qml/KotodamaMenu.qml"),
    )
    .files(["src/bridge.rs"]);
    // Qt loads a plugin through two C functions that moc defines, but a Rust
    // cdylib exports only what Rust defines, and drops the rest as unused.
    // moc's are renamed here, and src/plugin.rs exports the real names.
    // SAFETY: only adds defines.
    unsafe {
        builder.cc_builder(|cc| {
            cc.define("qt_plugin_instance", "kotodama_qt_plugin_instance");
            cc.define("qt_plugin_query_metadata_v2", "kotodama_qt_plugin_query_metadata_v2");
        })
    }
    .build();
}
