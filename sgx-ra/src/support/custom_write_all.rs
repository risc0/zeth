#![cfg(all(feature = "std", feature = "async"))]
use tokio::io::AsyncWrite;

use pin_project_lite::pin_project;
use std::future::Future;
use std::io;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::task::{Context, Poll};

pin_project! {
    /// A future returned by `custom_write_all` function that writes all data from a buffer to an AsyncWrite stream.
    ///
    /// This struct is used to control the behavior of writing data to the stream when it returns `Poll::Pending`.
    /// The buffer size will be adjusted based on the percentage specified in `buffer_change_percent`.
    #[derive(Debug)]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    pub struct CustomWriteAll<'a, W: ?Sized> {
        writer: &'a mut W,
        // The buffer containing the data to be written.
        buf: &'a [u8],
        // The left index of the current buffer.
        l: usize,
        // The right index of the current buffer (inclusive).
        r: usize,
        // The percentage number used to control how the buffer is changed when `poll_write` returns `Poll::Pending`.
        buffer_change_percent: i8,
        // Make this future `!Unpin` for compatibility with async trait methods.
        #[pin]
        _pin: PhantomPinned,
    }
}

/// This function will create a custom Future is for writing all data from a
/// buffer to an AsyncWrite stream. When [`AsyncWrite::poll_write`] returns
/// [`Poll::Pending`], this future will change the size of the buffer passed to
/// [`AsyncWrite::poll_write`] by `buffer_change_percent` percents.
///
/// This behavior is mainly for testing whether the underlying IO implementation
/// can resolve the limitation that comes from `mbedtls_ssl_write`. When
/// `mbedtls_ssl_write` returns `Error::SslWantWrite`, it needs to be called
/// again with the same arguments.
pub(crate) fn custom_write_all<'a, W>(writer: &'a mut W, buf: &'a [u8], buffer_change_percent: i8) -> CustomWriteAll<'a, W>
where
    W: AsyncWrite + Unpin + ?Sized,
{
    let min_len = 8 * 1024;
    assert!(buf.len() > min_len, "Please provide a buffer with length > {}", min_len);
    CustomWriteAll {
        writer,
        buf,
        l: 0,
        r: min_len,
        buffer_change_percent,
        _pin: PhantomPinned,
    }
}

/// This custom Future is for writing all data from a buffer to an AsyncWrite
/// stream. When [`AsyncWrite::poll_write`] returns [`Poll::Pending`], this
/// future will change the size of the buffer passed to
/// [`AsyncWrite::poll_write`].
///
/// This behavior is mainly for testing whether the underlying IO implementation
/// can resolve the limitation that comes from `mbedtls_ssl_write`. When
/// `mbedtls_ssl_write` returns `Error::SslWantWrite`, it needs to be called
/// again with the same arguments.
impl<W> Future for CustomWriteAll<'_, W>
where
    W: AsyncWrite + Unpin + ?Sized,
{
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let me = self.project();
        while me.l < me.r {
            let buf_len = me.buf.len();
            match Pin::new(&mut *me.writer).poll_write(cx, &me.buf[*me.l..*me.r]) {
                Poll::Ready(Ok(n)) => {
                    if n == 0 {
                        return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                    }
                    *me.l += n;
                    *me.r += n;
                    if *me.r > buf_len {
                        *me.r = buf_len;
                    }
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => {
                    *me.r = *me.l + (*me.r - *me.l) * (100 + *me.buffer_change_percent) as usize / 100;
                    if *me.r > buf_len {
                        *me.r = buf_len;
                    }
                    return Poll::Pending;
                }
            }
        }

        Poll::Ready(Ok(()))
    }
}
