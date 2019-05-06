#![allow(unused)] // Different tests use a different subset of functions

use bytes::Bytes;
use futures::{
    executor::{block_on, block_on_stream},
    io::{AsyncBufRead, AsyncRead, AsyncReadExt},
    stream::{self, Stream},
};
use pin_project::unsafe_project;
use pin_utils::pin_mut;
use proptest_derive::Arbitrary;
use std::{
    io::{self, Cursor, Read},
    pin::Pin,
    task::{Context, Poll},
    vec,
};

#[derive(Arbitrary, Debug)]
pub struct InputStream(Vec<Vec<u8>>);

impl InputStream {
    pub fn stream(&self) -> impl Stream<Item = io::Result<Bytes>> {
        // The resulting stream here will interleave empty chunks before and after each chunk, and
        // then interleave a `Poll::Pending` between each yielded chunk, that way we test the
        // handling of these two conditions in every point of the tested stream.
        PendStream::new(stream::iter(
            self.0
                .clone()
                .into_iter()
                .map(Bytes::from)
                .flat_map(|bytes| vec![Bytes::new(), bytes])
                .chain(Some(Bytes::new()))
                .map(Ok),
        ))
    }

    pub fn reader(&self) -> impl AsyncBufRead {
        // TODO: By using the stream here we ensure that each chunk will require a separate
        // read/poll_fill_buf call to process to help test reading multiple chunks. This is
        // blocked on having AsyncBufRead implemented on IntoAsyncRead:
        // (https://github.com/rust-lang-nursery/futures-rs/pull/1575)
        //
        // PendRead::new(self.stream().into_async_read())
        PendRead::new(Cursor::new(self.bytes()))
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.0.iter().flatten().cloned().collect()
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

#[unsafe_project(Unpin)]
struct PendStream<S>(bool, #[pin] S);

/// Injects at least one Poll::Pending in between each item
impl<S: Stream> PendStream<S> {
    fn new(stream: S) -> PendStream<S> {
        PendStream(false, stream)
    }
}

impl<S: Stream> Stream for PendStream<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        if *this.0 {
            let next = this.1.poll_next(cx);
            if next.is_ready() {
                *this.0 = false;
            }
            next
        } else {
            cx.waker().wake_by_ref();
            *this.0 = true;
            Poll::Pending
        }
    }
}

#[unsafe_project(Unpin)]
struct PendRead<R>(bool, #[pin] R);

/// Injects at least one Poll::Pending in between each ready read
impl<R: AsyncBufRead> PendRead<R> {
    fn new(reader: R) -> PendRead<R> {
        PendRead(false, reader)
    }
}

impl<R: AsyncRead> AsyncRead for PendRead<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        if *this.0 {
            let next = this.1.poll_read(cx, buf);
            if next.is_ready() {
                *this.0 = false;
            }
            next
        } else {
            cx.waker().wake_by_ref();
            *this.0 = true;
            Poll::Pending
        }
    }
}

impl<R: AsyncBufRead> AsyncBufRead for PendRead<R> {
    fn poll_fill_buf<'a>(
        self: Pin<&'a mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<&'a [u8]>> {
        let this = self.project();
        if *this.0 {
            let next = this.1.poll_fill_buf(cx);
            if next.is_ready() {
                *this.0 = false;
            }
            next
        } else {
            cx.waker().wake_by_ref();
            *this.0 = true;
            Poll::Pending
        }
    }

    fn consume(self: Pin<&mut Self>, amount: usize) {
        self.project().1.consume(amount);
    }
}

fn read_to_vec(mut read: impl Read) -> Vec<u8> {
    let mut output = vec![];
    read.read_to_end(&mut output).unwrap();
    output
}

fn async_read_to_vec(mut read: impl AsyncRead) -> Vec<u8> {
    let mut output = vec![];
    pin_mut!(read);
    block_on(read.read_to_end(&mut output)).unwrap();
    output
}

fn stream_to_vec(stream: impl Stream<Item = io::Result<Bytes>>) -> Vec<u8> {
    pin_mut!(stream);
    block_on_stream(stream)
        .map(Result::unwrap)
        .flatten()
        .collect()
}

pub fn brotli_compress(bytes: &[u8]) -> Vec<u8> {
    use brotli2::bufread::BrotliEncoder;
    read_to_vec(BrotliEncoder::new(bytes, 1))
}

pub fn brotli_decompress(bytes: &[u8]) -> Vec<u8> {
    use brotli2::bufread::BrotliDecoder;
    read_to_vec(BrotliDecoder::new(bytes))
}

pub fn brotli_stream_compress(input: impl Stream<Item = io::Result<Bytes>>) -> Vec<u8> {
    use async_compression::stream::brotli::{BrotliStream, Compress};
    pin_mut!(input);
    stream_to_vec(BrotliStream::new(input, Compress::new()))
}

pub fn brotli_stream_decompress(input: impl Stream<Item = io::Result<Bytes>>) -> Vec<u8> {
    use async_compression::stream::brotli::DecompressedBrotliStream;
    pin_mut!(input);
    stream_to_vec(DecompressedBrotliStream::new(input))
}

pub fn deflate_compress(bytes: &[u8]) -> Vec<u8> {
    use flate2::{bufread::DeflateEncoder, Compression};
    read_to_vec(DeflateEncoder::new(bytes, Compression::fast()))
}

pub fn deflate_decompress(bytes: &[u8]) -> Vec<u8> {
    use flate2::bufread::DeflateDecoder;
    read_to_vec(DeflateDecoder::new(bytes))
}

pub fn deflate_stream_compress(input: impl Stream<Item = io::Result<Bytes>>) -> Vec<u8> {
    use async_compression::stream::deflate::{Compression, DeflateStream};
    pin_mut!(input);
    stream_to_vec(DeflateStream::new(input, Compression::fast()))
}

pub fn deflate_stream_decompress(input: impl Stream<Item = io::Result<Bytes>>) -> Vec<u8> {
    use async_compression::stream::deflate::DecompressedDeflateStream;
    pin_mut!(input);
    stream_to_vec(DecompressedDeflateStream::new(input))
}

pub fn deflate_read_compress(input: impl AsyncBufRead) -> Vec<u8> {
    use async_compression::read::deflate::{Compression, DeflateRead};
    pin_mut!(input);
    async_read_to_vec(DeflateRead::new(input, Compression::fast()))
}

pub fn zlib_compress(bytes: &[u8]) -> Vec<u8> {
    use flate2::{bufread::ZlibEncoder, Compression};
    read_to_vec(ZlibEncoder::new(bytes, Compression::fast()))
}

pub fn zlib_decompress(bytes: &[u8]) -> Vec<u8> {
    use flate2::bufread::ZlibDecoder;
    read_to_vec(ZlibDecoder::new(bytes))
}

pub fn zlib_stream_compress(input: impl Stream<Item = io::Result<Bytes>>) -> Vec<u8> {
    use async_compression::stream::zlib::{Compression, ZlibStream};
    pin_mut!(input);
    stream_to_vec(ZlibStream::new(input, Compression::fast()))
}

pub fn zlib_stream_decompress(input: impl Stream<Item = io::Result<Bytes>>) -> Vec<u8> {
    use async_compression::stream::zlib::DecompressedZlibStream;
    pin_mut!(input);
    stream_to_vec(DecompressedZlibStream::new(input))
}

pub fn zlib_read_compress(input: impl AsyncBufRead) -> Vec<u8> {
    use async_compression::read::zlib::{Compression, ZlibRead};
    pin_mut!(input);
    async_read_to_vec(ZlibRead::new(input, Compression::fast()))
}

pub fn gzip_compress(bytes: &[u8]) -> Vec<u8> {
    use flate2::{bufread::GzEncoder, Compression};
    read_to_vec(GzEncoder::new(bytes, Compression::fast()))
}

pub fn gzip_decompress(bytes: &[u8]) -> Vec<u8> {
    use flate2::bufread::GzDecoder;
    read_to_vec(GzDecoder::new(bytes))
}

pub fn gzip_stream_compress(input: impl Stream<Item = io::Result<Bytes>>) -> Vec<u8> {
    use async_compression::stream::gzip::{Compression, GzipStream};
    pin_mut!(input);
    stream_to_vec(GzipStream::new(input, Compression::fast()))
}

pub fn gzip_stream_decompress(input: impl Stream<Item = io::Result<Bytes>>) -> Vec<u8> {
    use async_compression::stream::gzip::DecompressedGzipStream;
    pin_mut!(input);
    stream_to_vec(DecompressedGzipStream::new(input))
}