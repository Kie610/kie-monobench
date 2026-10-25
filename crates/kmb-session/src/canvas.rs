//! 下絵(キャンバス)の絵を画面に出す画素に戻す。画面は絵の形の crate を直に使わず、ここを通す
//! (画面の外枠はコアとセッションだけを知る)。

/// 文書に入れた絵(base64)を RGBA(幅, 高さ, 画素)に読む。長い辺が `max_side` を超えれば縮める。
pub fn decode(base64: &str, max_side: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let bytes = kmb_format::base64::decode(base64).ok_or("base64 を読めません")?;
    decode_bytes(&bytes, max_side)
}

/// 絵のファイルの中身(PNG など)を RGBA(幅, 高さ, 画素)に読む。長い辺が `max_side` を超えれば縮める。
/// 上の帯の印(アプリのアイコン)もここを通す。
pub fn decode_bytes(bytes: &[u8], max_side: u32) -> Result<(u32, u32, Vec<u8>), String> {
    kmb_image::decode_rgba(bytes, max_side)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_stored_picture_comes_back_as_pixels() {
        let png = kmb_format::png::write_rgba(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255]);
        let (w, h, px) = super::decode(&kmb_format::base64::encode(&png), 16).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(&px[4..8], &[0, 0, 255, 255]);
        assert!(super::decode("@@", 16).is_err());
    }
}
