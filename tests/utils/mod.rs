#![allow(dead_code, unused_macros)] // Different tests use a different subset of functions

#[cfg(feature = "tokio-02")]
mod tokio_02_ext;
#[cfg(feature = "futures-io")]
mod track_closed;

use proptest_derive::Arbitrary;

#[derive(Arbitrary, Debug, Clone)]
pub struct InputStream(Vec<Vec<u8>>);

impl InputStream {
    pub fn as_ref(&self) -> &[Vec<u8>] {
        &self.0
    }

    #[cfg(feature = "stream")]
    pub fn stream(&self) -> impl futures::stream::Stream<Item = std::io::Result<bytes::Bytes>> {
        use bytes::Bytes;
        use futures_test::stream::StreamTestExt;

        // The resulting stream here will interleave empty chunks before and after each chunk, and
        // then interleave a `Poll::Pending` between each yielded chunk, that way we test the
        // handling of these two conditions in every point of the tested stream.
        futures::stream::iter(
            self.0
                .clone()
                .into_iter()
                .map(Bytes::from)
                .flat_map(|bytes| vec![Bytes::new(), bytes])
                .chain(Some(Bytes::new()))
                .map(Ok),
        )
        .interleave_pending()
    }

    #[cfg(feature = "futures-io")]
    pub fn reader(&self) -> impl futures::io::AsyncBufRead {
        use futures::stream::TryStreamExt;
        use futures_test::stream::StreamTestExt;
        // TODO: By using the stream here we ensure that each chunk will require a separate
        // read/poll_fill_buf call to process to help test reading multiple chunks.
        futures::stream::iter(
            self.0
                .clone()
                .into_iter()
                .flat_map(|bytes| vec![Vec::new(), bytes])
                .chain(Some(Vec::new()))
                .map(Ok),
        )
        .interleave_pending()
        .into_async_read()
    }

    #[cfg(feature = "tokio-02")]
    pub fn tokio_reader(&self) -> impl tokio_02::io::AsyncBufRead {
        use bytes::Bytes;
        use futures_test::stream::StreamTestExt;
        // TODO: By using the stream here we ensure that each chunk will require a separate
        // read/poll_fill_buf call to process to help test reading multiple chunks.
        tokio_02::io::stream_reader(
            futures::stream::iter(
                self.0
                    .clone()
                    .into_iter()
                    .map(Bytes::from)
                    .flat_map(|bytes| vec![Bytes::new(), bytes])
                    .chain(Some(Bytes::new()))
                    .map(Ok),
            )
            .interleave_pending(),
        )
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.0.iter().flatten().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.0.iter().map(Vec::len).sum()
    }
}

// This happens to be the only dimension we're using
impl From<[[u8; 3]; 2]> for InputStream {
    fn from(input: [[u8; 3]; 2]) -> InputStream {
        InputStream(vec![Vec::from(&input[0][..]), Vec::from(&input[1][..])])
    }
}
impl From<Vec<Vec<u8>>> for InputStream {
    fn from(input: Vec<Vec<u8>>) -> InputStream {
        InputStream(input)
    }
}

pub mod prelude {
    pub use async_compression::Level;
    #[cfg(feature = "stream")]
    pub use bytes::Bytes;
    #[cfg(feature = "futures-io")]
    pub use futures::io::{AsyncBufRead, AsyncRead, AsyncWrite};
    #[cfg(feature = "stream")]
    pub use futures::stream::{self, Stream};
    pub use futures::{
        executor::{block_on, block_on_stream},
        pin_mut,
    };
    pub use std::{
        io::{self, Read},
        pin::Pin,
    };
    #[cfg(feature = "tokio-02")]
    pub use tokio_02::io::{
        AsyncBufRead as TokioBufRead, AsyncRead as TokioRead, AsyncWrite as TokioWrite,
    };

    pub fn read_to_vec(mut read: impl Read) -> Vec<u8> {
        let mut output = vec![];
        read.read_to_end(&mut output).unwrap();
        output
    }

    #[cfg(feature = "futures-io")]
    pub fn async_read_to_vec(read: impl AsyncRead) -> Vec<u8> {
        // TODO: https://github.com/rust-lang-nursery/futures-rs/issues/1510
        // All current test cases are < 100kB
        let mut output = futures::io::Cursor::new(vec![0; 102_400]);
        pin_mut!(read);
        let len = block_on(futures::io::copy_buf(
            futures::io::BufReader::with_capacity(2, read),
            &mut output,
        ))
        .unwrap();
        let mut output = output.into_inner();
        output.truncate(len as usize);
        output
    }

    #[cfg(feature = "futures-io")]
    pub fn async_write_to_vec(
        input: &[Vec<u8>],
        create_writer: impl for<'a> FnOnce(
            &'a mut (dyn AsyncWrite + Unpin),
        ) -> Pin<Box<dyn AsyncWrite + 'a>>,
        limit: usize,
    ) -> Vec<u8> {
        use crate::utils::track_closed::TrackClosed;
        use futures::io::AsyncWriteExt as _;
        use futures_test::io::AsyncWriteTestExt as _;

        let mut output = Vec::new();
        {
            let mut test_writer = TrackClosed::new(
                (&mut output)
                    .limited_write(limit)
                    .interleave_pending_write(),
            );
            {
                let mut writer = create_writer(&mut test_writer);
                for chunk in input {
                    block_on(writer.write_all(chunk)).unwrap();
                    block_on(writer.flush()).unwrap();
                }
                block_on(writer.close()).unwrap();
            }
            assert!(test_writer.is_closed());
        }
        output
    }

    #[cfg(feature = "stream")]
    pub fn stream_to_vec(stream: impl Stream<Item = io::Result<Bytes>>) -> Vec<u8> {
        pin_mut!(stream);
        block_on_stream(stream)
            .map(Result::unwrap)
            .flatten()
            .collect()
    }

    #[cfg(feature = "tokio-02")]
    pub fn tokio_read_to_vec(read: impl TokioRead) -> Vec<u8> {
        let mut output = std::io::Cursor::new(vec![0; 102_400]);
        pin_mut!(read);
        let len = block_on(crate::utils::tokio_02_ext::copy_buf(
            tokio_02::io::BufReader::with_capacity(2, read),
            &mut output,
        ))
        .unwrap();
        let mut output = output.into_inner();
        output.truncate(len as usize);
        output
    }

    #[cfg(feature = "tokio-02")]
    pub fn tokio_write_to_vec(
        input: &[Vec<u8>],
        create_writer: impl for<'a> FnOnce(
            &'a mut (dyn TokioWrite + Unpin),
        ) -> Pin<Box<dyn TokioWrite + 'a>>,
        limit: usize,
    ) -> Vec<u8> {
        use crate::utils::tokio_02_ext::AsyncWriteTestExt;
        use crate::utils::track_closed::TrackClosed;
        use tokio_02::io::AsyncWriteExt as _;

        let mut output = std::io::Cursor::new(Vec::new());
        {
            let mut test_writer = TrackClosed::new(
                (&mut output)
                    .limited_write(limit)
                    .interleave_pending_write(),
            );
            {
                let mut writer = create_writer(&mut test_writer);
                for chunk in input {
                    block_on(writer.write_all(chunk)).unwrap();
                    block_on(writer.flush()).unwrap();
                }
                block_on(writer.shutdown()).unwrap();
            }
            assert!(test_writer.is_closed());
        }
        output.into_inner()
    }
}

macro_rules! algos {
    ($(pub mod $name:ident($feat:literal, $encoder:ident, $decoder:ident) { pub mod sync { $($tt:tt)* } })*) => {
        $(
            #[cfg(feature = $feat)]
            pub mod $name {
                pub mod sync { $($tt)* }

                #[cfg(feature = "stream")]
                pub mod stream {
                    use crate::utils::prelude::*;
                    pub use async_compression::stream::{$decoder as Decoder, $encoder as Encoder};

                    pub fn compress(input: impl Stream<Item = io::Result<Bytes>>) -> Vec<u8> {
                        pin_mut!(input);
                        stream_to_vec(Encoder::with_quality(input, Level::Fastest))
                    }

                    pub fn decompress(input: impl Stream<Item = io::Result<Bytes>>) -> Vec<u8> {
                        pin_mut!(input);
                        stream_to_vec(Decoder::new(input))
                    }
                }

                #[cfg(feature = "futures-io")]
                pub mod futures {
                    pub mod bufread {
                        use crate::utils::prelude::*;
                        pub use async_compression::futures::bufread::{
                            $decoder as Decoder, $encoder as Encoder,
                        };

                        pub fn compress(input: impl AsyncBufRead) -> Vec<u8> {
                            pin_mut!(input);
                            async_read_to_vec(Encoder::with_quality(input, Level::Fastest))
                        }

                        pub fn decompress(input: impl AsyncBufRead) -> Vec<u8> {
                            pin_mut!(input);
                            async_read_to_vec(Decoder::new(input))
                        }
                    }

                    pub mod write {
                        use crate::utils::prelude::*;
                        pub use async_compression::futures::write::{
                            $decoder as Decoder, $encoder as Encoder,
                        };

                        pub fn compress(input: &[Vec<u8>], limit: usize) -> Vec<u8> {
                            async_write_to_vec(
                                input,
                                |input| Box::pin(Encoder::with_quality(input, Level::Fastest)),
                                limit,
                            )
                        }

                        pub fn decompress(input: &[Vec<u8>], limit: usize) -> Vec<u8> {
                            async_write_to_vec(input, |input| Box::pin(Decoder::new(input)), limit)
                        }
                    }
                }

                #[cfg(feature = "tokio-02")]
                pub mod tokio_02 {
                    pub mod bufread {
                        use crate::utils::prelude::*;
                        pub use async_compression::tokio_02::bufread::{
                            $decoder as Decoder, $encoder as Encoder,
                        };

                        pub fn compress(input: impl TokioBufRead) -> Vec<u8> {
                            pin_mut!(input);
                            tokio_read_to_vec(Encoder::with_quality(input, Level::Fastest))
                        }

                        pub fn decompress(input: impl TokioBufRead) -> Vec<u8> {
                            pin_mut!(input);
                            tokio_read_to_vec(Decoder::new(input))
                        }
                    }

                    pub mod write {
                        use crate::utils::prelude::*;
                        pub use async_compression::tokio_02::write::{
                            $decoder as Decoder, $encoder as Encoder,
                        };

                        pub fn compress(input: &[Vec<u8>], limit: usize) -> Vec<u8> {
                            tokio_write_to_vec(
                                input,
                                |input| Box::pin(Encoder::with_quality(input, Level::Fastest)),
                                limit,
                            )
                        }

                        pub fn decompress(input: &[Vec<u8>], limit: usize) -> Vec<u8> {
                            tokio_write_to_vec(input, |input| Box::pin(Decoder::new(input)), limit)
                        }
                    }
                }
            }
        )*
    }
}

algos! {
    pub mod brotli("brotli", BrotliEncoder, BrotliDecoder) {
        pub mod sync {
            use crate::utils::prelude::*;

            pub fn compress(bytes: &[u8]) -> Vec<u8> {
                use brotli::{enc::backward_references::BrotliEncoderParams, CompressorReader};
                let mut params = BrotliEncoderParams::default();
                params.quality = 1;
                read_to_vec(CompressorReader::with_params(bytes, 0, &params))
            }

            pub fn decompress(bytes: &[u8]) -> Vec<u8> {
                use brotli::Decompressor;
                read_to_vec(Decompressor::new(bytes, 0))
            }
        }
    }

    pub mod bzip2("bzip2", BzEncoder, BzDecoder) {
        pub mod sync {
            use crate::utils::prelude::*;

            pub fn compress(bytes: &[u8]) -> Vec<u8> {
                use bzip2::{bufread::BzEncoder, Compression};
                read_to_vec(BzEncoder::new(bytes, Compression::fast()))
            }

            pub fn decompress(bytes: &[u8]) -> Vec<u8> {
                use bzip2::bufread::BzDecoder;
                read_to_vec(BzDecoder::new(bytes))
            }
        }
    }

    pub mod deflate("deflate", DeflateEncoder, DeflateDecoder) {
        pub mod sync {
            use crate::utils::prelude::*;

            pub fn compress(bytes: &[u8]) -> Vec<u8> {
                use flate2::{bufread::DeflateEncoder, Compression};
                read_to_vec(DeflateEncoder::new(bytes, Compression::fast()))
            }

            pub fn decompress(bytes: &[u8]) -> Vec<u8> {
                use flate2::bufread::DeflateDecoder;
                read_to_vec(DeflateDecoder::new(bytes))
            }
        }
    }

    pub mod zlib("zlib", ZlibEncoder, ZlibDecoder) {
        pub mod sync {
            use crate::utils::prelude::*;

            pub fn compress(bytes: &[u8]) -> Vec<u8> {
                use flate2::{bufread::ZlibEncoder, Compression};
                read_to_vec(ZlibEncoder::new(bytes, Compression::fast()))
            }

            pub fn decompress(bytes: &[u8]) -> Vec<u8> {
                use flate2::bufread::ZlibDecoder;
                read_to_vec(ZlibDecoder::new(bytes))
            }
        }
    }

    pub mod gzip("gzip", GzipEncoder, GzipDecoder) {
        pub mod sync {
            use crate::utils::prelude::*;

            pub fn compress(bytes: &[u8]) -> Vec<u8> {
                use flate2::{bufread::GzEncoder, Compression};
                read_to_vec(GzEncoder::new(bytes, Compression::fast()))
            }

            pub fn decompress(bytes: &[u8]) -> Vec<u8> {
                use flate2::bufread::GzDecoder;
                read_to_vec(GzDecoder::new(bytes))
            }
        }
    }

    pub mod zstd("zstd", ZstdEncoder, ZstdDecoder) {
        pub mod sync {
            use crate::utils::prelude::*;

            pub fn compress(bytes: &[u8]) -> Vec<u8> {
                use libzstd::stream::read::Encoder;
                use libzstd::DEFAULT_COMPRESSION_LEVEL;
                read_to_vec(Encoder::new(bytes, DEFAULT_COMPRESSION_LEVEL).unwrap())
            }

            pub fn decompress(bytes: &[u8]) -> Vec<u8> {
                use libzstd::stream::read::Decoder;
                read_to_vec(Decoder::new(bytes).unwrap())
            }
        }
    }

    pub mod xz("xz", XzEncoder, XzDecoder) {
        pub mod sync {
            use crate::utils::prelude::*;

            pub fn compress(bytes: &[u8]) -> Vec<u8> {
                use xz2::bufread::XzEncoder;

                read_to_vec(XzEncoder::new(bytes, 0))
            }

            pub fn decompress(bytes: &[u8]) -> Vec<u8> {
                use xz2::bufread::XzDecoder;

                read_to_vec(XzDecoder::new(bytes))
            }
        }
    }

    pub mod lzma("lzma", LzmaEncoder, LzmaDecoder) {
        pub mod sync {
            use crate::utils::prelude::*;

            pub fn compress(bytes: &[u8]) -> Vec<u8> {
                use xz2::bufread::XzEncoder;
                use xz2::stream::{LzmaOptions, Stream};

                read_to_vec(XzEncoder::new_stream(
                    bytes,
                    Stream::new_lzma_encoder(&LzmaOptions::new_preset(0).unwrap()).unwrap(),
                ))
            }

            pub fn decompress(bytes: &[u8]) -> Vec<u8> {
                use xz2::bufread::XzDecoder;
                use xz2::stream::Stream;

                read_to_vec(XzDecoder::new_stream(
                    bytes,
                    Stream::new_lzma_decoder(u64::max_value()).unwrap(),
                ))
            }
        }
    }
}

macro_rules! test_cases {
    ($variant:ident) => {
        mod $variant {
            #[cfg(feature = "stream")]
            mod stream {
                mod compress {
                    use crate::utils::{self, prelude::*};
                    use futures::{executor::block_on, stream::StreamExt as _};
                    use std::iter::FromIterator;

                    #[test]
                    #[ntest::timeout(1000)]
                    fn empty() {
                        // Can't use InputStream for this as it will inject extra empty chunks
                        let compressed =
                            utils::$variant::stream::compress(futures::stream::empty());
                        let output = utils::$variant::sync::decompress(&compressed);

                        assert_eq!(output, &[][..]);
                    }

                    #[test]
                    #[ntest::timeout(1000)]
                    fn empty_chunk() {
                        let input = utils::InputStream::from(vec![vec![]]);

                        let compressed = utils::$variant::stream::compress(input.stream());
                        let output = utils::$variant::sync::decompress(&compressed);

                        assert_eq!(output, input.bytes());
                    }

                    #[test]
                    #[ntest::timeout(1000)]
                    fn short() {
                        let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                        let compressed = utils::$variant::stream::compress(input.stream());
                        let output = utils::$variant::sync::decompress(&compressed);

                        assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                    }

                    #[test]
                    #[ntest::timeout(1000)]
                    fn long() {
                        let input = vec![
                            Vec::from_iter((0..32_768).map(|_| rand::random())),
                            Vec::from_iter((0..32_768).map(|_| rand::random())),
                        ];
                        let input = utils::InputStream::from(input);

                        let compressed = utils::$variant::stream::compress(input.stream());
                        let output = utils::$variant::sync::decompress(&compressed);

                        assert_eq!(output, input.bytes());
                    }

                    #[test]
                    #[ntest::timeout(1000)]
                    fn error() {
                        let err = std::io::Error::new(std::io::ErrorKind::Other, "failure");
                        let input = futures::stream::iter(vec![Err(err)]);

                        let mut stream =
                            utils::$variant::stream::Encoder::with_quality(input, Level::Fastest);

                        assert!(block_on(stream.next()).unwrap().is_err());
                        assert!(block_on(stream.next()).is_none());
                    }

                    #[test]
                    fn with_level_0() {
                        let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                        let encoder = utils::$variant::stream::Encoder::with_quality(
                            input.stream(),
                            Level::Precise(0),
                        );
                        let compressed = stream_to_vec(encoder);
                        let output = utils::$variant::sync::decompress(&compressed);

                        assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                    }

                    #[test]
                    fn with_level_max() {
                        let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                        let encoder = utils::$variant::stream::Encoder::with_quality(
                            input.stream(),
                            Level::Precise(u32::max_value()),
                        );
                        let compressed = stream_to_vec(encoder);
                        let output = utils::$variant::sync::decompress(&compressed);

                        assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                    }
                }

                mod decompress {
                    use crate::utils;
                    use futures::{executor::block_on, stream::StreamExt as _};
                    use std::iter::FromIterator;

                    #[test]
                    #[ntest::timeout(1000)]
                    fn empty() {
                        let compressed = utils::$variant::sync::compress(&[]);

                        let stream = utils::InputStream::from(vec![compressed]);
                        let output = utils::$variant::stream::decompress(stream.stream());

                        assert_eq!(output, &[][..]);
                    }

                    #[test]
                    #[ntest::timeout(1000)]
                    fn short() {
                        let compressed = utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]);

                        let stream = utils::InputStream::from(vec![compressed]);
                        let output = utils::$variant::stream::decompress(stream.stream());

                        assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                    }

                    #[test]
                    #[ntest::timeout(1000)]
                    fn long() {
                        let input = Vec::from_iter((0..65_536).map(|_| rand::random()));
                        let compressed = utils::$variant::sync::compress(&input);

                        let stream = utils::InputStream::from(vec![compressed]);
                        let output = utils::$variant::stream::decompress(stream.stream());

                        assert_eq!(output, input);
                    }

                    #[test]
                    #[ntest::timeout(1000)]
                    fn long_chunks() {
                        let input = Vec::from_iter((0..65_536).map(|_| rand::random()));
                        let compressed = utils::$variant::sync::compress(&input);

                        let stream = utils::InputStream::from(
                            compressed.chunks(1024).map(Vec::from).collect::<Vec<_>>(),
                        );
                        let output = utils::$variant::stream::decompress(stream.stream());

                        assert_eq!(output, input);
                    }

                    #[test]
                    #[ntest::timeout(1000)]
                    fn trailer() {
                        // Currently there is no way to get any partially consumed stream item from
                        // the decoder, for now we just guarantee that if the compressed frame
                        // exactly matches an item boundary we will not read the next item from the
                        // stream.
                        let compressed = utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]);

                        let stream = utils::InputStream::from(vec![compressed, vec![7, 8, 9, 10]]);

                        let mut stream = stream.stream();
                        let output = utils::$variant::stream::decompress(&mut stream);
                        let trailer = utils::prelude::stream_to_vec(stream);

                        assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        assert_eq!(trailer, &[7, 8, 9, 10][..]);
                    }

                    #[test]
                    #[ntest::timeout(1000)]
                    fn multiple_members() {
                        let compressed = [
                            utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]),
                            utils::$variant::sync::compress(&[6, 5, 4, 3, 2, 1]),
                        ]
                        .join(&[][..]);

                        let stream = utils::InputStream::from(vec![compressed]);

                        let mut decoder = utils::$variant::stream::Decoder::new(stream.stream());
                        decoder.multiple_members(true);
                        let output = utils::prelude::stream_to_vec(decoder);

                        assert_eq!(output, &[1, 2, 3, 4, 5, 6, 6, 5, 4, 3, 2, 1][..]);
                    }

                    #[test]
                    #[ntest::timeout(1000)]
                    fn multiple_members_chunked() {
                        let compressed = [
                            utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]),
                            utils::$variant::sync::compress(&[6, 5, 4, 3, 2, 1]),
                        ]
                        .join(&[][..]);

                        let stream = utils::InputStream::from(
                            compressed.chunks(1).map(Vec::from).collect::<Vec<_>>(),
                        );

                        let mut decoder = utils::$variant::stream::Decoder::new(stream.stream());
                        decoder.multiple_members(true);
                        let output = utils::prelude::stream_to_vec(decoder);

                        assert_eq!(output, &[1, 2, 3, 4, 5, 6, 6, 5, 4, 3, 2, 1][..]);
                    }

                    #[test]
                    #[ntest::timeout(1000)]
                    fn error() {
                        let err = std::io::Error::new(std::io::ErrorKind::Other, "failure");
                        let input = futures::stream::iter(vec![Err(err)]);

                        let mut stream = utils::$variant::stream::Decoder::new(input);

                        assert!(block_on(stream.next()).unwrap().is_err());
                        assert!(block_on(stream.next()).is_none());
                    }

                    #[test]
                    #[ntest::timeout(1000)]
                    fn invalid_data() {
                        let input = futures::stream::iter(vec![Ok(bytes::Bytes::from(
                            &[1, 2, 3, 4, 5, 6][..],
                        ))]);

                        let mut stream = utils::$variant::stream::Decoder::new(input);

                        assert!(block_on(stream.next()).unwrap().is_err());
                        assert!(block_on(stream.next()).is_none());
                    }
                }
            }

            #[cfg(feature = "futures-io")]
            mod futures {
                mod bufread {
                    mod compress {
                        use crate::utils::{self, prelude::*};
                        use std::iter::FromIterator;

                        #[test]
                        #[ntest::timeout(1000)]
                        fn empty() {
                            let mut input: &[u8] = &[];
                            let compressed =
                                utils::$variant::futures::bufread::compress(&mut input);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn empty_chunk() {
                            let input = utils::InputStream::from(vec![vec![]]);

                            let compressed =
                                utils::$variant::futures::bufread::compress(input.reader());
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, input.bytes());
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let compressed =
                                utils::$variant::futures::bufread::compress(input.reader());
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long() {
                            let input = vec![
                                Vec::from_iter((0..32_768).map(|_| rand::random())),
                                Vec::from_iter((0..32_768).map(|_| rand::random())),
                            ];
                            let input = utils::InputStream::from(input);

                            let compressed =
                                utils::$variant::futures::bufread::compress(input.reader());
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, input.bytes());
                        }

                        #[test]
                        fn with_level_0() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let encoder = utils::$variant::futures::bufread::Encoder::with_quality(
                                input.reader(),
                                Level::Precise(0),
                            );
                            let compressed = async_read_to_vec(encoder);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        fn with_level_max() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let encoder = utils::$variant::futures::bufread::Encoder::with_quality(
                                input.reader(),
                                Level::Precise(u32::max_value()),
                            );
                            let compressed = async_read_to_vec(encoder);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }
                    }

                    mod decompress {
                        use crate::utils::{self, prelude::*};
                        use std::iter::FromIterator;

                        #[test]
                        #[ntest::timeout(1000)]
                        fn empty() {
                            let compressed = utils::$variant::sync::compress(&[]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output =
                                utils::$variant::futures::bufread::decompress(stream.reader());

                            assert_eq!(output, &[][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn zeros() {
                            let compressed = utils::$variant::sync::compress(&[0; 10]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output =
                                utils::$variant::futures::bufread::decompress(stream.reader());

                            assert_eq!(output, &[0; 10][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short() {
                            let compressed = utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output =
                                utils::$variant::futures::bufread::decompress(stream.reader());

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short_chunks() {
                            let compressed = utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]);

                            let stream = utils::InputStream::from(
                                compressed.chunks(2).map(Vec::from).collect::<Vec<_>>(),
                            );
                            let output =
                                utils::$variant::futures::bufread::decompress(stream.reader());

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn trailer() {
                            let mut compressed =
                                utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]);

                            compressed.extend_from_slice(&[7, 8, 9, 10]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let mut reader = stream.reader();
                            let output = utils::$variant::futures::bufread::decompress(&mut reader);
                            let trailer = async_read_to_vec(reader);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                            assert_eq!(trailer, &[7, 8, 9, 10][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long() {
                            let input = Vec::from_iter((0..65_536).map(|_| rand::random()));
                            let compressed = utils::$variant::sync::compress(&input);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output =
                                utils::$variant::futures::bufread::decompress(stream.reader());

                            assert_eq!(output, input);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long_chunks() {
                            let input = Vec::from_iter((0..65_536).map(|_| rand::random()));
                            let compressed = utils::$variant::sync::compress(&input);

                            let stream = utils::InputStream::from(
                                compressed.chunks(1024).map(Vec::from).collect::<Vec<_>>(),
                            );
                            let output =
                                utils::$variant::futures::bufread::decompress(stream.reader());

                            assert_eq!(output, input);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn multiple_members() {
                            let compressed = [
                                utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]),
                                utils::$variant::sync::compress(&[6, 5, 4, 3, 2, 1]),
                            ]
                            .join(&[][..]);

                            let stream = utils::InputStream::from(vec![compressed]);

                            let mut decoder =
                                utils::$variant::futures::bufread::Decoder::new(stream.reader());
                            decoder.multiple_members(true);
                            let output = utils::prelude::async_read_to_vec(decoder);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6, 6, 5, 4, 3, 2, 1][..]);
                        }
                    }
                }

                mod write {
                    mod compress {
                        use crate::utils::{self, prelude::*};
                        use std::iter::FromIterator;

                        #[test]
                        #[ntest::timeout(1000)]
                        fn empty() {
                            let input = utils::InputStream::from(vec![]);
                            let compressed =
                                utils::$variant::futures::write::compress(input.as_ref(), 65_536);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn empty_chunk() {
                            let input = utils::InputStream::from(vec![vec![]]);

                            let compressed =
                                utils::$variant::futures::write::compress(input.as_ref(), 65_536);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, input.bytes());
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let compressed =
                                utils::$variant::futures::write::compress(input.as_ref(), 65_536);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short_chunk_output() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let compressed =
                                utils::$variant::futures::write::compress(input.as_ref(), 2);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long() {
                            let input = vec![
                                Vec::from_iter((0..32_768).map(|_| rand::random())),
                                Vec::from_iter((0..32_768).map(|_| rand::random())),
                            ];
                            let input = utils::InputStream::from(input);

                            let compressed =
                                utils::$variant::futures::write::compress(input.as_ref(), 65_536);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, input.bytes());
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long_chunk_output() {
                            let input = vec![
                                Vec::from_iter((0..32_768).map(|_| rand::random())),
                                Vec::from_iter((0..32_768).map(|_| rand::random())),
                            ];
                            let input = utils::InputStream::from(input);

                            let compressed =
                                utils::$variant::futures::write::compress(input.as_ref(), 20);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, input.bytes());
                        }

                        #[test]
                        fn with_level_0() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let compressed = async_write_to_vec(
                                input.as_ref(),
                                |input| {
                                    Box::pin(
                                        utils::$variant::futures::write::Encoder::with_quality(
                                            input,
                                            Level::Precise(0),
                                        ),
                                    )
                                },
                                65_536,
                            );
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        fn with_level_max() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let compressed = async_write_to_vec(
                                input.as_ref(),
                                |input| {
                                    Box::pin(
                                        utils::$variant::futures::write::Encoder::with_quality(
                                            input,
                                            Level::Precise(u32::max_value()),
                                        ),
                                    )
                                },
                                65_536,
                            );
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }
                    }

                    mod decompress {
                        use crate::utils;
                        use std::iter::FromIterator;

                        #[test]
                        #[ntest::timeout(1000)]
                        fn empty() {
                            let compressed = utils::$variant::sync::compress(&[]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output = utils::$variant::futures::write::decompress(
                                stream.as_ref(),
                                65_536,
                            );

                            assert_eq!(output, &[][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn zeros() {
                            let compressed = utils::$variant::sync::compress(&[0; 10]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output = utils::$variant::futures::write::decompress(
                                stream.as_ref(),
                                65_536,
                            );

                            assert_eq!(output, &[0; 10][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short() {
                            let compressed = utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output = utils::$variant::futures::write::decompress(
                                stream.as_ref(),
                                65_536,
                            );

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short_chunks() {
                            let compressed = utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]);

                            let stream = utils::InputStream::from(
                                compressed.chunks(2).map(Vec::from).collect::<Vec<_>>(),
                            );
                            let output = utils::$variant::futures::write::decompress(
                                stream.as_ref(),
                                65_536,
                            );

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long() {
                            let input = Vec::from_iter((0..65_536).map(|_| rand::random()));
                            let compressed = utils::$variant::sync::compress(&input);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output = utils::$variant::futures::write::decompress(
                                stream.as_ref(),
                                65_536,
                            );

                            assert_eq!(output, input);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long_chunks() {
                            let input = Vec::from_iter((0..65_536).map(|_| rand::random()));
                            let compressed = utils::$variant::sync::compress(&input);

                            let stream = utils::InputStream::from(
                                compressed.chunks(1024).map(Vec::from).collect::<Vec<_>>(),
                            );
                            let output = utils::$variant::futures::write::decompress(
                                stream.as_ref(),
                                65_536,
                            );

                            assert_eq!(output, input);
                        }
                    }
                }
            }

            #[cfg(feature = "tokio-02")]
            mod tokio_02 {
                mod bufread {
                    mod compress {
                        use crate::utils::{self, prelude::*};
                        use std::iter::FromIterator;

                        #[test]
                        #[ntest::timeout(1000)]
                        fn empty() {
                            let mut input: &[u8] = &[];
                            let compressed =
                                utils::$variant::tokio_02::bufread::compress(&mut input);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn empty_chunk() {
                            let input = utils::InputStream::from(vec![vec![]]);

                            let compressed =
                                utils::$variant::tokio_02::bufread::compress(input.tokio_reader());
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, input.bytes());
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let compressed =
                                utils::$variant::tokio_02::bufread::compress(input.tokio_reader());
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long() {
                            let input = vec![
                                Vec::from_iter((0..32_768).map(|_| rand::random())),
                                Vec::from_iter((0..32_768).map(|_| rand::random())),
                            ];
                            let input = utils::InputStream::from(input);

                            let compressed =
                                utils::$variant::tokio_02::bufread::compress(input.tokio_reader());
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, input.bytes());
                        }

                        #[test]
                        fn with_level_0() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let encoder = utils::$variant::tokio_02::bufread::Encoder::with_quality(
                                input.tokio_reader(),
                                Level::Precise(0),
                            );
                            let compressed = tokio_read_to_vec(encoder);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        fn with_level_max() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let encoder = utils::$variant::tokio_02::bufread::Encoder::with_quality(
                                input.tokio_reader(),
                                Level::Precise(u32::max_value()),
                            );
                            let compressed = tokio_read_to_vec(encoder);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }
                    }

                    mod decompress {
                        use crate::utils::{self, prelude::*};
                        use std::iter::FromIterator;

                        #[test]
                        #[ntest::timeout(1000)]
                        fn empty() {
                            let compressed = utils::$variant::sync::compress(&[]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output = utils::$variant::tokio_02::bufread::decompress(
                                stream.tokio_reader(),
                            );

                            assert_eq!(output, &[][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn zeros() {
                            let compressed = utils::$variant::sync::compress(&[0; 10]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output = utils::$variant::tokio_02::bufread::decompress(
                                stream.tokio_reader(),
                            );

                            assert_eq!(output, &[0; 10][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short() {
                            let compressed = utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output = utils::$variant::tokio_02::bufread::decompress(
                                stream.tokio_reader(),
                            );

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short_chunks() {
                            let compressed = utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]);

                            let stream = utils::InputStream::from(
                                compressed.chunks(2).map(Vec::from).collect::<Vec<_>>(),
                            );
                            let output = utils::$variant::tokio_02::bufread::decompress(
                                stream.tokio_reader(),
                            );

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn trailer() {
                            let mut compressed =
                                utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]);

                            compressed.extend_from_slice(&[7, 8, 9, 10]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let mut reader = stream.tokio_reader();
                            let output =
                                utils::$variant::tokio_02::bufread::decompress(&mut reader);
                            let trailer = tokio_read_to_vec(reader);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                            assert_eq!(trailer, &[7, 8, 9, 10][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long() {
                            let input = Vec::from_iter((0..65_536).map(|_| rand::random()));
                            let compressed = utils::$variant::sync::compress(&input);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output = utils::$variant::tokio_02::bufread::decompress(
                                stream.tokio_reader(),
                            );

                            assert_eq!(output, input);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long_chunks() {
                            let input = Vec::from_iter((0..65_536).map(|_| rand::random()));
                            let compressed = utils::$variant::sync::compress(&input);

                            let stream = utils::InputStream::from(
                                compressed.chunks(1024).map(Vec::from).collect::<Vec<_>>(),
                            );
                            let output = utils::$variant::tokio_02::bufread::decompress(
                                stream.tokio_reader(),
                            );

                            assert_eq!(output, input);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn multiple_members() {
                            let compressed = [
                                utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]),
                                utils::$variant::sync::compress(&[6, 5, 4, 3, 2, 1]),
                            ]
                            .join(&[][..]);

                            let stream = utils::InputStream::from(vec![compressed]);

                            let mut decoder = utils::$variant::tokio_02::bufread::Decoder::new(
                                stream.tokio_reader(),
                            );
                            decoder.multiple_members(true);
                            let output = utils::prelude::tokio_read_to_vec(decoder);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6, 6, 5, 4, 3, 2, 1][..]);
                        }
                    }
                }

                mod write {
                    mod compress {
                        use crate::utils::{self, prelude::*};
                        use std::iter::FromIterator;

                        #[test]
                        #[ntest::timeout(1000)]
                        fn empty() {
                            let input = utils::InputStream::from(vec![]);
                            let compressed =
                                utils::$variant::tokio_02::write::compress(input.as_ref(), 65_536);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn empty_chunk() {
                            let input = utils::InputStream::from(vec![vec![]]);

                            let compressed =
                                utils::$variant::tokio_02::write::compress(input.as_ref(), 65_536);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, input.bytes());
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let compressed =
                                utils::$variant::tokio_02::write::compress(input.as_ref(), 65_536);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short_chunk_output() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let compressed =
                                utils::$variant::tokio_02::write::compress(input.as_ref(), 2);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long() {
                            let input = vec![
                                Vec::from_iter((0..32_768).map(|_| rand::random())),
                                Vec::from_iter((0..32_768).map(|_| rand::random())),
                            ];
                            let input = utils::InputStream::from(input);

                            let compressed =
                                utils::$variant::tokio_02::write::compress(input.as_ref(), 65_536);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, input.bytes());
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long_chunk_output() {
                            let input = vec![
                                Vec::from_iter((0..32_768).map(|_| rand::random())),
                                Vec::from_iter((0..32_768).map(|_| rand::random())),
                            ];
                            let input = utils::InputStream::from(input);

                            let compressed =
                                utils::$variant::tokio_02::write::compress(input.as_ref(), 20);
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, input.bytes());
                        }

                        #[test]
                        fn with_level_0() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let compressed = tokio_write_to_vec(
                                input.as_ref(),
                                |input| {
                                    Box::pin(
                                        utils::$variant::tokio_02::write::Encoder::with_quality(
                                            input,
                                            Level::Precise(0),
                                        ),
                                    )
                                },
                                65_536,
                            );
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        fn with_level_max() {
                            let input = utils::InputStream::from([[1, 2, 3], [4, 5, 6]]);

                            let compressed = tokio_write_to_vec(
                                input.as_ref(),
                                |input| {
                                    Box::pin(
                                        utils::$variant::tokio_02::write::Encoder::with_quality(
                                            input,
                                            Level::Precise(u32::max_value()),
                                        ),
                                    )
                                },
                                65_536,
                            );
                            let output = utils::$variant::sync::decompress(&compressed);

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }
                    }

                    mod decompress {
                        use crate::utils;
                        use std::iter::FromIterator;

                        #[test]
                        #[ntest::timeout(1000)]
                        fn empty() {
                            let compressed = utils::$variant::sync::compress(&[]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output = utils::$variant::tokio_02::write::decompress(
                                stream.as_ref(),
                                65_536,
                            );

                            assert_eq!(output, &[][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn zeros() {
                            let compressed = utils::$variant::sync::compress(&[0; 10]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output = utils::$variant::tokio_02::write::decompress(
                                stream.as_ref(),
                                65_536,
                            );

                            assert_eq!(output, &[0; 10][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short() {
                            let compressed = utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output = utils::$variant::tokio_02::write::decompress(
                                stream.as_ref(),
                                65_536,
                            );

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn short_chunks() {
                            let compressed = utils::$variant::sync::compress(&[1, 2, 3, 4, 5, 6]);

                            let stream = utils::InputStream::from(
                                compressed.chunks(2).map(Vec::from).collect::<Vec<_>>(),
                            );
                            let output = utils::$variant::tokio_02::write::decompress(
                                stream.as_ref(),
                                65_536,
                            );

                            assert_eq!(output, &[1, 2, 3, 4, 5, 6][..]);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long() {
                            let input = Vec::from_iter((0..65_536).map(|_| rand::random()));
                            let compressed = utils::$variant::sync::compress(&input);

                            let stream = utils::InputStream::from(vec![compressed]);
                            let output = utils::$variant::tokio_02::write::decompress(
                                stream.as_ref(),
                                65_536,
                            );

                            assert_eq!(output, input);
                        }

                        #[test]
                        #[ntest::timeout(1000)]
                        fn long_chunks() {
                            let input = Vec::from_iter((0..65_536).map(|_| rand::random()));
                            let compressed = utils::$variant::sync::compress(&input);

                            let stream = utils::InputStream::from(
                                compressed.chunks(1024).map(Vec::from).collect::<Vec<_>>(),
                            );
                            let output = utils::$variant::tokio_02::write::decompress(
                                stream.as_ref(),
                                65_536,
                            );

                            assert_eq!(output, input);
                        }
                    }
                }
            }
        }
    };
}
