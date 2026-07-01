#[test]
fn app_exposes_the_product_entrypoint() {
    let _: fn() = mzed::app::run;
}
