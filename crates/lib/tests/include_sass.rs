#[cfg(feature = "macro")]
#[tokio::test]
async fn basic() {
    let css: &str = grass::include!("./input.scss");

    assert_eq!(css, "a{color:red}");
}
