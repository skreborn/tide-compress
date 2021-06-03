#[async_std::test]
async fn all() {
    assert!(matches!(
        tide_compress::CompressionLevels::all(tide_compress::Level::Fastest),
        tide_compress::CompressionLevels {
            brotli: tide_compress::Level::Fastest,
            deflate: tide_compress::Level::Fastest,
            gzip: tide_compress::Level::Fastest,
        },
    ));

    assert!(matches!(
        tide_compress::CompressionLevels::all(tide_compress::Level::Best),
        tide_compress::CompressionLevels {
            brotli: tide_compress::Level::Best,
            deflate: tide_compress::Level::Best,
            gzip: tide_compress::Level::Best,
        },
    ));
}
