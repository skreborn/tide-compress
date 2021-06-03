#[cfg(feature = "brotli")]
use async_compression::futures::bufread::BrotliEncoder;
#[cfg(feature = "deflate")]
use async_compression::futures::bufread::DeflateEncoder;
#[cfg(feature = "gzip")]
use async_compression::futures::bufread::GzipEncoder;
use async_compression::Level;
use futures_lite::io::BufReader;
use tide::http::cache::{CacheControl, CacheDirective};
use tide::http::conditional::Vary;
use tide::http::content::{AcceptEncoding, ContentEncoding, Encoding};
use tide::http::{headers, Body, Method};
use tide::{Middleware, Next, Request, Response};

const THRESHOLD: usize = 1024;

/// The configured compression level for all available compression algorithms.
///
/// ## Example
/// ```rust
/// # async_std::task::block_on(async {
/// let mut app = tide::new();
///
/// let levels = tide_compress::CompressionLevels {
///     brotli: tide_compress::Level::Precise(4),
///     gzip: tide_compress::Level::Fastest,
///     deflate: tide_compress::Level::Default,
/// };
///
/// app.with(tide_compress::CompressMiddleware::with_levels(levels));
/// # })
/// ```
#[derive(Clone, Debug)]
pub struct CompressionLevels {
    #[cfg(feature = "brotli")]
    pub brotli: Level,
    #[cfg(feature = "gzip")]
    pub gzip: Level,
    #[cfg(feature = "deflate")]
    pub deflate: Level,
}

impl Default for CompressionLevels {
    fn default() -> Self {
        CompressionLevels {
            #[cfg(feature = "brotli")]
            brotli: Level::Default,
            #[cfg(feature = "gzip")]
            gzip: Level::Default,
            #[cfg(feature = "deflate")]
            deflate: Level::Default,
        }
    }
}

impl CompressionLevels {
    /// Creates a new compression level configuration with the same level set for all available
    /// compression algorithms.
    ///
    /// ## Example
    /// ```rust
    /// # async_std::task::block_on(async {
    /// let mut app = tide::new();
    ///
    /// let levels = tide_compress::CompressionLevels::all(tide_compress::Level::Fastest);
    ///
    /// app.with(tide_compress::CompressMiddleware::with_levels(levels));
    /// # })
    /// ```
    pub fn all(level: Level) -> Self {
        CompressionLevels {
            #[cfg(feature = "brotli")]
            brotli: level,
            #[cfg(feature = "gzip")]
            gzip: level,
            #[cfg(feature = "deflate")]
            deflate: level,
        }
    }
}

/// A middleware for compressing response body data.
///
/// ## Example
/// ```rust
/// # async_std::task::block_on(async {
/// let mut app = tide::new();
///
/// app.with(tide_compress::CompressMiddleware::new());
/// # })
/// ```
#[derive(Clone, Debug)]
pub struct CompressMiddleware {
    threshold: usize,
    levels: CompressionLevels,
}

impl Default for CompressMiddleware {
    fn default() -> Self {
        CompressMiddleware {
            threshold: THRESHOLD,
            levels: CompressionLevels::default(),
        }
    }
}

impl CompressMiddleware {
    /// Creates a new CompressMiddleware.
    ///
    /// Uses the default minimum body size threshold (1024 bytes) and compression levels.
    ///
    /// ## Example
    /// ```rust
    /// # async_std::task::block_on(async {
    /// let mut app = tide::new();
    ///
    /// app.with(tide_compress::CompressMiddleware::new());
    /// # })
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new CompressMiddleware with a custom minimum body size threshold.
    ///
    /// Uses the default compression levels.
    ///
    /// # Arguments
    ///
    /// * `threshold` - minimum body size in bytes.
    ///
    /// ## Example
    /// ```rust
    /// # async_std::task::block_on(async {
    /// let mut app = tide::new();
    ///
    /// app.with(tide_compress::CompressMiddleware::with_threshold(512));
    /// # })
    /// ```
    pub fn with_threshold(threshold: usize) -> Self {
        CompressMiddleware {
            threshold,
            ..CompressMiddleware::default()
        }
    }

    /// Creates a new CompressMiddleware with custom compression levels.
    ///
    /// Uses the default minimum body size threshold (1024 bytes).
    ///
    /// # Arguments
    ///
    /// * `levels` - desired compression levels.
    ///
    /// ## Example
    /// ```rust
    /// # async_std::task::block_on(async {
    /// let mut app = tide::new();
    ///
    /// let levels = tide_compress::CompressionLevels {
    ///     brotli: tide_compress::Level::Precise(4),
    ///     gzip: tide_compress::Level::Fastest,
    ///     deflate: tide_compress::Level::Default,
    /// };
    ///
    /// app.with(tide_compress::CompressMiddleware::with_levels(levels));
    /// # })
    /// ```
    pub fn with_levels(levels: CompressionLevels) -> Self {
        CompressMiddleware {
            levels,
            ..CompressMiddleware::default()
        }
    }

    /// Creates a new CompressMiddleware with a custom minimum body size threshold and compression
    /// levels.
    ///
    /// # Arguments
    ///
    /// * `threshold` - minimum body size in bytes.
    /// * `levels` - desired compression levels.
    ///
    /// ## Example
    /// ```rust
    /// # async_std::task::block_on(async {
    /// let mut app = tide::new();
    ///
    /// let levels = tide_compress::CompressionLevels {
    ///     brotli: tide_compress::Level::Precise(4),
    ///     gzip: tide_compress::Level::Fastest,
    ///     deflate: tide_compress::Level::Default,
    /// };
    ///
    /// app.with(tide_compress::CompressMiddleware::with_threshold_and_levels(512, levels));
    /// # })
    /// ```
    pub fn with_threshold_and_levels(threshold: usize, levels: CompressionLevels) -> Self {
        CompressMiddleware { threshold, levels }
    }
}

#[tide::utils::async_trait]
impl<State: Clone + Send + Sync + 'static> Middleware<State> for CompressMiddleware {
    async fn handle(&self, req: Request<State>, next: Next<'_, State>) -> tide::Result {
        // Incoming Request data
        // Need to grab these things before the request is consumed by `next.run()`.
        let is_head = req.method() == Method::Head;
        let accepts = AcceptEncoding::from_headers(&req)?;

        // Propagate to route
        let mut res: Response = next.run(req).await;

        // Head requests should have no body to compress.
        // Can't tell if we can compress if there is no Accepts-Encoding header.
        if is_head || accepts.is_none() {
            return Ok(res);
        }
        let mut accepts = accepts.unwrap();

        // Should we transform?
        if let Some(cache_control) = CacheControl::from_headers(&res)? {
            // No compression for `Cache-Control: no-transform`
            // https://tools.ietf.org/html/rfc7234#section-5.2.2.4
            if cache_control
                .iter()
                .any(|directive| directive == &CacheDirective::NoTransform)
            {
                return Ok(res);
            }
        }

        // Set the Vary header, similar to how https://www.npmjs.com/package/compression does it.
        let mut vary = Vary::new();
        vary.push(headers::ACCEPT_ENCODING)?;
        vary.apply(&mut res);

        // Check if an encoding may already exist.
        // Can't tell if we should compress if an encoding set.
        if let Some(previous_encoding) = ContentEncoding::from_headers(&res)? {
            if previous_encoding != Encoding::Identity {
                return Ok(res);
            }
        }

        // Check body length against threshold.
        if let Some(body_len) = res.len() {
            if body_len < self.threshold {
                return Ok(res);
            }
        }

        let body = res.take_body();
        let encoding = accepts.negotiate(&[
            #[cfg(feature = "brotli")]
            Encoding::Brotli,
            #[cfg(feature = "gzip")]
            Encoding::Gzip,
            #[cfg(feature = "deflate")]
            Encoding::Deflate,
        ])?;

        // Get a new Body backed by an appropriate encoder, if one is available.
        res.set_body(get_encoder(body, &encoding, &self.levels));
        encoding.apply(&mut res);

        // End size no longer matches body size, so any existing Content-Length is useless.
        res.remove_header(headers::CONTENT_LENGTH);

        Ok(res)
    }
}

/// Returns a `Body` made from an encoder chosen from the `Encoding`.
fn get_encoder(body: Body, encoding: &ContentEncoding, levels: &CompressionLevels) -> Body {
    #[cfg(feature = "brotli")]
    {
        if *encoding == Encoding::Brotli {
            return Body::from_reader(
                BufReader::new(BrotliEncoder::with_quality(body, levels.brotli)),
                None,
            );
        }
    }

    #[cfg(feature = "gzip")]
    {
        if *encoding == Encoding::Gzip {
            return Body::from_reader(
                BufReader::new(GzipEncoder::with_quality(body, levels.gzip)),
                None,
            );
        }
    }

    #[cfg(feature = "deflate")]
    {
        if *encoding == Encoding::Deflate {
            return Body::from_reader(
                BufReader::new(DeflateEncoder::with_quality(body, levels.deflate)),
                None,
            );
        }
    }

    body
}
