fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        slint_build::compile_with_config(
            "ui/main.slint",
            slint_build::CompilerConfiguration::new().with_style("fluent-dark".into()),
        )
        .expect("Could not compile Slint UI");
    }
}
