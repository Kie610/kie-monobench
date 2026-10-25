//! 受け取る 1 行(1 つの送り物)の大きさの上限。MCP の部品は改行まで際限なく溜めるので、壊れた
//! クライアントが改行の無い大きな送り物を流すと記憶を食い尽くす(採用分 10-24 m3)。上限を超えたら
//! 読み取りを誤りにして接続を終える(動いている画面への接続の上限 `kmb_session::live` と同じ 16 MB)。

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};

/// 1 行の上限(バイト)。
pub const MAX_LINE: usize = 16 << 20;

/// 改行の無いまま `max` バイトを超えて読むと誤りを返す読み手。
pub struct LineCap<R> {
    inner: R,
    max: usize,
    /// 最後の改行から読んだバイト数
    run: usize,
}

impl<R> LineCap<R> {
    pub fn new(inner: R, max: usize) -> Self {
        Self { inner, max, run: 0 }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for LineCap<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let start = buf.filled().len();
        let me = &mut *self;
        if let Err(e) = std::task::ready!(Pin::new(&mut me.inner).poll_read(cx, buf)) {
            return Poll::Ready(Err(e));
        }
        for &b in &buf.filled()[start..] {
            me.run = if b == b'\n' { 0 } else { me.run + 1 };
            if me.run > me.max {
                log::warn!(
                    "MCP: {} バイトを超える送り物を受けたので接続を終える",
                    me.max
                );
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("1 つの送り物が {} バイトを超えました", me.max),
                )));
            }
        }
        Poll::Ready(Ok(()))
    }
}
