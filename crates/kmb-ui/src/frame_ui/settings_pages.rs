//! 設定の窓の頁のうち、画面の枠が持つ物(マウス・3D のビュー・自動保存・読み込み・配置・描画)。

use super::*;

impl Workspace {
    /// 設定の「マウス」の節。
    pub(crate) fn mouse_settings(&mut self, ui: &mut egui::Ui) {
        ui.weak("左=選ぶ、右クリック=メニュー、右ドラッグ=回す、ホイール=拡大(カーソルの位置へ)");
        ui.horizontal(|ui| {
            ui.label("ホイール押しドラッグ");
            ui.radio_value(&mut self.win.layout.middle_drag, MiddleDrag::Pan, "動かす");
            ui.radio_value(&mut self.win.layout.middle_drag, MiddleDrag::Orbit, "回す");
        });
        ui.weak("Shift を押しながらだと、もう片方になる。Ctrl を押しながらだと拡大");
        ui.horizontal(|ui| {
            ui.label("オービットのタイプ");
            ui.radio_value(&mut self.win.layout.free_orbit, false, "拘束オービット");
            ui.radio_value(&mut self.win.layout.free_orbit, true, "フリーオービット");
        });
        ui.weak("拘束オービットは Z を上に保って回す。フリーオービットは上下の軸に縛らず、真上を越えて回せる(ビューの下の帯のオービットを右クリックでも替えられる)");
        let state = self.win.mouse3d.as_ref().map_or_else(
            || "探していない(安全モードでは探さない)".to_string(),
            |m| m.status().label(),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label("3D マウス");
            ui.weak(state);
        });
    }

    /// 設定の「3D の表示」の節。
    pub(crate) fn view3d_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("表示スタイル");
            for s in crate::Shading::ALL {
                ui.radio_value(&mut self.win.layout.shading, s, s.label());
            }
        });
        ui.horizontal(|ui| {
            ui.label("投影");
            for p in crate::Projection::ALL {
                ui.radio_value(&mut self.win.layout.projection, p, p.label())
                    .on_hover_text(p.hint());
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("効果");
            self.effects_items(ui);
        });
        ui.weak("ビューの右上のボタンと、何もない所の右クリックでも替えられる");
        ui.checkbox(&mut self.win.layout.show_fps, "画面の速さを出す")
            .on_hover_text(
                "3D のビューの右下に、直近 1 秒に描いた回数(fps)と、1 回の処理にかかった時間\n\
                 (そのうち 3D のビューを描く時間と、面や辺を拾う時間)を出す。\n\
                 止まっている間は描き直さないので、fps は動かしている間の値を見る",
            );
    }

    /// 設定の「自動保存」の節。
    pub(crate) fn autosave_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("間隔");
            let name = |m: u32| match m {
                0 => "しない".to_string(),
                m => format!("{m} 分ごと"),
            };
            egui::ComboBox::from_id_salt("autosave_minutes")
                .selected_text(name(self.win.layout.autosave_minutes))
                .show_ui(ui, |ui| {
                    for m in crate::layout::AUTOSAVE_CHOICES {
                        ui.selectable_value(&mut self.win.layout.autosave_minutes, m, name(m));
                    }
                })
                .response
                .on_hover_text(
                    "変えた文書の写しを、この間隔でデータフォルダに書く(落ちた時に開始画面から戻せる)。\n\
                     大きな文書で保存が重い時は延ばす。「しない」は自分で保存する人向け",
                );
        });
    }

    /// 設定の「読み込み」の節。
    pub(crate) fn import_settings(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(
            &mut self.win.layout.ask_step_up,
            "STEP を読み込んだら、立てるかを聞く",
        )
        .on_hover_text(
            "他の CAD(Y が上)で作った形は寝た向きで入るので、読み込んだすぐ後に\n\
             「立てますか」と 1 回聞く。機能の欄の「Y が上の形を立てる」でもいつでも変えられる",
        );
    }

    /// 設定の「画面の配置」の節。
    pub(crate) fn arrangement_settings(&mut self, ui: &mut egui::Ui) {
        ui.weak(
            "3D ビュー・ブラウザ・編集パネル・タイムラインの並びと開閉、ツールバーのピンとタブの順を覚えている。\n\
             部品はタブを引いて上下左右へ付け替えたり、重ねたり、窓に浮かせたりできる",
        );
        if ui
            .add_enabled(
                !self.dock_is_default()
                    || self.win.layout != {
                        let mut d = self.win.layout.clone();
                        d.reset_arrangement();
                        d
                    },
                egui::Button::new("配置を既定に戻す"),
            )
            .on_hover_text("部品の並びとツールバーのピン・タブの順を、初めの形に戻す")
            .clicked()
        {
            self.win.layout.reset_arrangement();
            self.reset_dock();
        }
        ui.add_space(6.0);
        ui.add_enabled(
            !self.win.safe_mode,
            egui::Checkbox::new(
                &mut self.win.layout.system_title_bar,
                "窓の帯を Windows の標準にする",
            ),
        )
        .on_hover_text(
            "切ると、上の帯の右端に最小化・最大化・閉じるを置き、帯の空いた所を引いて窓を動かす(既定)。\n\
             入れると Windows の標準の帯に戻す。安全モードでは標準の帯",
        );
    }

    /// 設定の窓の「描画」(U26)。上でまとめての質を選び、下の項目を替えると「自分で」になる。
    pub(crate) fn render_settings(&mut self, ui: &mut egui::Ui) {
        use crate::view3d_render::{
            Api, Device, MSAA_CHOICES, MeshDetail, Preset, SCALE_CHOICES, View3dMode,
        };
        let before = self.win.layout.render.clone();
        let r = &mut self.win.layout.render;
        ui.horizontal_wrapped(|ui| {
            ui.label("まとめて");
            let now = r.preset();
            for p in Preset::CHOICES {
                if ui.radio(now == p, p.label()).clicked() {
                    r.apply(p);
                }
            }
            if now == Preset::Custom {
                ui.label(egui::RichText::new(Preset::Custom.label()).strong());
            }
        });
        ui.weak(
            "軽さ優先は細かさ 75 %・アンチエイリアスなし・粗い網。見た目優先は 150 %・細かい網",
        );
        ui.horizontal(|ui| {
            ui.label("3D のビューの描き方");
            for m in View3dMode::ALL {
                ui.radio_value(&mut r.view3d, m, m.label());
            }
        })
        .response
        .on_hover_text(
            "自動は、ソフトウェアの描画装置(WARP・llvmpipe)なら CPU、ほかは GPU で描く。\n\
             GPU で描いて表示がおかしい時は CPU にする(すぐ替わる)",
        );
        ui.horizontal(|ui| {
            ui.label("描く細かさ");
            egui::ComboBox::from_id_salt("render_scale")
                .selected_text(format!("{} %", r.scale))
                .show_ui(ui, |ui| {
                    for v in SCALE_CHOICES {
                        ui.selectable_value(&mut r.scale, v, format!("{v} %"));
                    }
                });
        })
        .response
        .on_hover_text(
            "3D のビューを画面の画素の何 % で描くか。下げると速く、上げると線がなめらか",
        );
        ui.horizontal(|ui| {
            ui.label("アンチエイリアス(GPU)");
            for v in MSAA_CHOICES {
                ui.radio_value(
                    &mut r.msaa,
                    v,
                    if v == 1 {
                        "なし".to_string()
                    } else {
                        format!("{v} 倍")
                    },
                );
            }
        });
        ui.checkbox(&mut r.dynamic, "回す・動かす間は軽く描く(CPU で描く時)")
            .on_hover_text("動かしている間だけ細かさを半分にし、止まると元の細かさで描き直す");
        ui.checkbox(&mut r.ao, "入り隅を暗くする(GPU で描く時)")
            .on_hover_text("面と面が向き合う隅に陰を付け、形の奥行きを見やすくする(環境光の遮蔽)");
        ui.horizontal(|ui| {
            ui.label("形の網の細かさ");
            egui::ComboBox::from_id_salt("render_mesh")
                .selected_text(r.mesh.label())
                .show_ui(ui, |ui| {
                    for m in MeshDetail::ALL {
                        ui.selectable_value(&mut r.mesh, m, m.label());
                    }
                });
        })
        .response
        .on_hover_text(
            "細かいほど小さな穴やフィレットが角張らない。網を作り直す時間とメモリが増える。\n\
             測る道具は画面の網で測るので、細かさで値がわずかに変わる",
        );
        ui.horizontal(|ui| {
            ui.label("隠れた辺の濃さ");
            ui.add(egui::Slider::new(&mut r.hidden_dim, 0..=100).suffix(" %"));
        })
        .response
        .on_hover_text(
            "表示の形「シェーディング、隠線エッジ表示」で、面に隠れた辺を描く点線の濃さ",
        );
        ui.horizontal(|ui| {
            ui.label("稜線の太さ");
            let (lo, hi) = crate::RenderSettings::EDGE_WIDTH;
            ui.add(
                egui::Slider::new(&mut r.edge_width, lo..=hi)
                    .step_by(0.1)
                    .suffix(" px"),
            );
        })
        .response
        .on_hover_text("ボディの辺を描く線の太さ(画面の画素)。既定は 1 px");
        ui.label(egui::RichText::new("次の起動から").weak());
        ui.horizontal(|ui| {
            ui.label("描画装置");
            egui::ComboBox::from_id_salt("render_device")
                .selected_text(r.device.label())
                .show_ui(ui, |ui| {
                    for d in Device::ALL {
                        ui.selectable_value(&mut r.device, d, d.label());
                    }
                });
        })
        .response
        .on_hover_text(
            "GPU が 2 つある PC で、高性能・省電力のどちらを使うか。ソフトウェアは GPU のドライバが\n\
             壊れた時の逃げ道(遅い)。安全モードで起動すると、ここに関わらずソフトウェアで描く",
        );
        ui.horizontal(|ui| {
            ui.label("描画の API");
            egui::ComboBox::from_id_salt("render_api")
                .selected_text(r.api.label())
                .show_ui(ui, |ui| {
                    for a in Api::ALL {
                        ui.selectable_value(&mut r.api, a, a.label());
                    }
                });
        })
        .response
        .on_hover_text(
            "画面を描く仕組み(Windows)。\n\
             DirectX 12(DXC): 既定。シェーダーを新しい DXC で作る(アプリの横の dxcompiler.dll。\n\
             無ければ FXC で作る)。DirectX 12 Ultimate の上位の機能も、これで使える形になる。\n\
             DirectX 12(FXC): Windows に入っている古い FXC で作る。DXC で困った時に。\n\
             Vulkan: DirectX 12 の具合が悪いドライバの時に。\n\
             選んだ API で窓を開けなければ、自動で選び直して開く。ほかの OS では自動で選ぶ",
        );
        if let Some(now) = &self.win.renderer_note {
            ui.weak(format!("今: {now}"));
        }
        let r = &mut self.win.layout.render;
        ui.checkbox(&mut r.vsync, "垂直同期")
            .on_hover_text("切ると画面の書き換えを待たず、fps の上限が外れる(速さを測る時)");
        if *r != before {
            log::info!("描画の設定: {:?}(まとめて {:?})", r, r.preset());
        }
    }
}
