//! アプリのアイコン(`assets/make_icon.py` で作る)。窓・exe のアイコンと、上の帯の左上の印は
//! ここにある同じ絵を使う(印だけ別に描くと、アイコンを替えた時に取り残される)。

/// 窓のアイコン(256 px)。アプリが窓に付ける
pub const PNG: &[u8] = include_bytes!("../assets/icon.png");
/// exe のアイコン(16〜256 px の絵を 1 つにまとめた物。各絵は PNG)。アプリの build.rs が exe に
/// 埋め込み、上の帯の印はここから映る大きさに合う絵を選ぶ(小さい絵ほど線を太く描いてある)
pub const ICO: &[u8] = include_bytes!("../assets/icon.ico");

/// `ico` の絵のうち、一辺が `px` 画素以上で最も小さい物(無ければ最も大きい物)の一辺と PNG。
pub(crate) fn frame(ico: &[u8], px: u32) -> Option<(u32, &[u8])> {
    let n = u16::from_le_bytes(ico.get(4..6)?.try_into().ok()?) as usize;
    let mut best: Option<(u32, &[u8])> = None;
    for i in 0..n {
        let e = ico.get(6 + 16 * i..22 + 16 * i)?;
        // 幅の 0 は 256
        let side = if e[0] == 0 { 256 } else { u32::from(e[0]) };
        let len = u32::from_le_bytes(e[8..12].try_into().ok()?) as usize;
        let at = u32::from_le_bytes(e[12..16].try_into().ok()?) as usize;
        let data = ico.get(at..at.checked_add(len)?)?;
        let better = match best {
            None => true,
            Some((b, _)) if b >= px => side >= px && side < b,
            Some((b, _)) => side > b,
        };
        if better {
            best = Some((side, data));
        }
    }
    best
}

/// 上の帯の印を `rect` に描く。絵は映る画素の大きさごとに 1 度だけ読み、egui の記憶に置く
/// (読めなかった事も置き、毎回は読み直さない)。
pub(crate) fn paint(ui: &egui::Ui, rect: egui::Rect) {
    let px = (rect.width().max(rect.height()) * ui.ctx().pixels_per_point()).ceil() as u32;
    let Some((side, png)) = frame(ICO, px) else {
        return;
    };
    let id = egui::Id::new("kmb_ui::app_icon").with(side);
    let tex = ui
        .ctx()
        .data(|d| d.get_temp::<Option<egui::TextureHandle>>(id));
    let tex = tex.unwrap_or_else(|| {
        let tex = match kmb_session::canvas::decode_bytes(png, 256) {
            Ok((w, h, rgba)) => Some(ui.ctx().load_texture(
                format!("app-icon-{side}"),
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                egui::TextureOptions::LINEAR,
            )),
            Err(e) => {
                log::warn!("アプリのアイコン({side} px)を読めません: {e}");
                None
            }
        };
        ui.ctx().data_mut(|d| d.insert_temp(id, tex.clone()));
        tex
    });
    if let Some(tex) = tex {
        ui.painter().image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_size_reads_and_the_band_picks_the_one_that_fits() {
        let (w, h, _) =
            kmb_session::canvas::decode_bytes(PNG, 256).expect("窓のアイコンが読めない");
        assert_eq!((w, h), (256, 256));
        for (px, want) in [
            (1, 16),
            (16, 16),
            (22, 24),
            (55, 64),
            (200, 256),
            (999, 256),
        ] {
            let (side, png) = frame(ICO, px).expect("exe のアイコンに絵が無い");
            assert_eq!(side, want, "{px} px に選んだ絵");
            let (w, h, _) =
                kmb_session::canvas::decode_bytes(png, 256).expect("exe のアイコンの絵が読めない");
            assert_eq!((w, h), (side, side));
        }
        assert!(frame(&ICO[..20], 24).is_none(), "切れたアイコンは読まない");
    }
}
