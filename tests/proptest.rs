use async_compression::Level;

use ::proptest::{
    arbitrary::any,
    prop_oneof,
    strategy::{Just, Strategy},
};

mod utils;

#[allow(dead_code)]
fn any_level() -> impl Strategy<Value = Level> {
    prop_oneof![
        Just(Level::Fastest),
        Just(Level::Best),
        Just(Level::Default),
        any::<u32>().prop_map(Level::Precise),
    ]
}

macro_rules! tests {
    ($($name:ident($feat:literal)),* $(,)?) => {
        $(
            #[cfg(feature = $feat)]
            mod $name {
                #[cfg(feature = "stream")]
                mod stream {
                    use crate::utils;
                    use proptest::{prelude::{any, ProptestConfig}, proptest};
                    use std::iter::FromIterator;

                    proptest! {
                        #[test]
                        fn compress(ref input in any::<utils::InputStream>()) {
                            let compressed = utils::$name::stream::compress(input.stream());
                            let output = utils::$name::sync::decompress(&compressed);
                            assert_eq!(output, input.bytes());
                        }

                        #[test]
                        fn decompress(
                            ref input in any::<Vec<u8>>(),
                            chunk_size in 1..20usize,
                        ) {
                            let compressed = utils::$name::sync::compress(input);
                            let stream = utils::InputStream::from(Vec::from_iter(compressed.chunks(chunk_size).map(Vec::from)));
                            let output = utils::$name::stream::decompress(stream.stream());
                            assert_eq!(&output, input);
                        }
                    }

                    proptest! {
                        #![proptest_config(ProptestConfig::with_cases(32))]

                        #[test]
                        fn compress_with_level(
                            ref input in any::<utils::InputStream>(),
                            level in crate::any_level(),
                        ) {
                            let encoder = utils::$name::stream::Encoder::with_quality(input.stream(), level);
                            let compressed = utils::prelude::stream_to_vec(encoder);
                            let output = utils::$name::sync::decompress(&compressed);
                            assert_eq!(output, input.bytes());
                        }
                    }
                }

                #[cfg(feature = "futures-io")]
                mod futures {
                    mod bufread {
                        use crate::utils;
                        use proptest::{prelude::{any, ProptestConfig}, proptest};
                        use std::iter::FromIterator;

                        proptest! {
                            #[test]
                            fn compress(ref input in any::<utils::InputStream>()) {
                                let compressed = utils::$name::futures::bufread::compress(input.reader());
                                let output = utils::$name::sync::decompress(&compressed);
                                assert_eq!(output, input.bytes());
                            }

                            #[test]
                            fn decompress(
                                ref input in any::<Vec<u8>>(),
                                chunk_size in 1..20usize,
                            ) {
                                let compressed = utils::$name::sync::compress(input);
                                let stream = utils::InputStream::from(Vec::from_iter(compressed.chunks(chunk_size).map(Vec::from)));
                                let output = utils::$name::futures::bufread::decompress(stream.reader());
                                assert_eq!(&output, input);
                            }
                        }

                        proptest! {
                            #![proptest_config(ProptestConfig::with_cases(32))]

                            #[test]
                            fn compress_with_level(
                                ref input in any::<utils::InputStream>(),
                                level in crate::any_level(),
                            ) {
                                let encoder = utils::$name::futures::bufread::Encoder::with_quality(input.reader(), level);
                                let compressed = utils::prelude::async_read_to_vec(encoder);
                                let output = utils::$name::sync::decompress(&compressed);
                                assert_eq!(output, input.bytes());
                            }
                        }
                    }

                    mod write {
                        use crate::utils;
                        use proptest::{prelude::{any, ProptestConfig}, proptest};

                        proptest! {
                            #[test]
                            fn compress(
                                ref input in any::<utils::InputStream>(),
                                limit in 1..20usize,
                            ) {
                                let compressed = utils::$name::futures::write::compress(input.as_ref(), limit);
                                let output = utils::$name::sync::decompress(&compressed);
                                assert_eq!(output, input.bytes());
                            }
                        }

                        proptest! {
                            #![proptest_config(ProptestConfig::with_cases(32))]

                            #[test]
                            fn compress_with_level(
                                ref input in any::<utils::InputStream>(),
                                limit in 1..20usize,
                                level in crate::any_level(),
                            ) {
                                let compressed = utils::prelude::async_write_to_vec(
                                    input.as_ref(),
                                    |input| Box::pin(utils::$name::futures::write::Encoder::with_quality(input, level)),
                                    limit,
                                );
                                let output = utils::$name::sync::decompress(&compressed);
                                assert_eq!(output, input.bytes());
                            }
                        }
                    }
                }

                #[cfg(feature = "tokio-02")]
                mod tokio_02 {
                    mod bufread {
                        use crate::utils;
                        use proptest::{prelude::{any, ProptestConfig}, proptest};
                        use std::iter::FromIterator;

                        proptest! {
                            #[test]
                            fn compress(ref input in any::<utils::InputStream>()) {
                                let compressed = utils::$name::tokio_02::bufread::compress(input.tokio_reader());
                                let output = utils::$name::sync::decompress(&compressed);
                                assert_eq!(output, input.bytes());
                            }

                            #[test]
                            fn decompress(
                                ref input in any::<Vec<u8>>(),
                                chunk_size in 1..20usize,
                            ) {
                                let compressed = utils::$name::sync::compress(input);
                                let stream = utils::InputStream::from(Vec::from_iter(compressed.chunks(chunk_size).map(Vec::from)));
                                let output = utils::$name::tokio_02::bufread::decompress(stream.tokio_reader());
                                assert_eq!(&output, input);
                            }
                        }

                        proptest! {
                            #![proptest_config(ProptestConfig::with_cases(32))]

                            #[test]
                            fn compress_with_level(
                                ref input in any::<utils::InputStream>(),
                                level in crate::any_level(),
                            ) {
                                let encoder = utils::$name::tokio_02::bufread::Encoder::with_quality(input.tokio_reader(), level);
                                let compressed = utils::prelude::tokio_read_to_vec(encoder);
                                let output = utils::$name::sync::decompress(&compressed);
                                assert_eq!(output, input.bytes());
                            }
                        }
                    }

                    mod write {
                        use crate::utils;
                        use proptest::{prelude::{any, ProptestConfig}, proptest};

                        proptest! {
                            #[test]
                            fn compress(
                                ref input in any::<utils::InputStream>(),
                                limit in 1..20usize,
                            ) {
                                let compressed = utils::$name::tokio_02::write::compress(input.as_ref(), limit);
                                let output = utils::$name::sync::decompress(&compressed);
                                assert_eq!(output, input.bytes());
                            }
                        }

                        proptest! {
                            #![proptest_config(ProptestConfig::with_cases(32))]

                            #[test]
                            fn compress_with_level(
                                ref input in any::<utils::InputStream>(),
                                limit in 1..20usize,
                                level in crate::any_level(),
                            ) {
                                let compressed = utils::prelude::tokio_write_to_vec(
                                    input.as_ref(),
                                    |input| Box::pin(utils::$name::tokio_02::write::Encoder::with_quality(input, level)),
                                    limit,
                                );
                                let output = utils::$name::sync::decompress(&compressed);
                                assert_eq!(output, input.bytes());
                            }
                        }
                    }
                }
            }
        )*
    }
}

mod proptest {
    tests! {
        brotli("brotli"),
        bzip2("bzip2"),
        deflate("deflate"),
        gzip("gzip"),
        lzma("lzma"),
        xz("xz"),
        zlib("zlib"),
        zstd("zstd"),
    }
}
