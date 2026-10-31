//! 3D のビューを GPU(wgpu)で描く。
//!
//! CPU の描画([`crate::view3d::render_guided`])と同じ絵を、画面の外の絵(MSAA 4 倍と深度つき)に
//! 描き、egui の画像として貼る。網は形が変わった時だけ GPU へ送り、回す・動かす・拡大の間は
//! 行列の値だけを替えて描き直す。面の色(選んだ面・マウスの下の面)は、選びが変わった時だけ送り直す。
//!
//! GPU が無い時(画面の試験、描画装置を作れなかった時)は、CPU の描画をそのまま使う。
//! 面を拾うのは、どちらでも CPU の計算([`crate::view3d::pick_at`]。ボディごとの箱の木で調べる)。
//! 辺を拾うのも、どちらでも CPU の計算([`crate::view3d::edge_at`]。隠れは面の箱の木で調べる)。
//! PC によらず同じ辺を拾うため、GPU の有無にかかわらず CPU で選ぶ。

use crate::Shading;
use crate::view3d::{Behind, Camera, Colors, Ghost, GhostLayer, Guides, Hover, Pick, Scene};
use egui::Rect;
use egui_wgpu::wgpu;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wgpu::util::DeviceExt;

/// 画面の外の絵の MSAA の数の既定(描画の設定の「アンチエイリアス」で 1 にもできる。U26)。
const SAMPLES: u32 = 4;
/// egui の画像と同じく、sRGB の値をそのまま持つ(egui が貼る時に変換する)
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// 入り隅の陰(環境光の遮蔽)の前の段で、画素ごとに見えている面の位置(mm)を残す絵
const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
/// 入り隅の陰を探す半径(場面の対角の長さに対する割合)、強さ、標本の数、法線の向きの偏り。
/// 試験アプリで見た目を合わせた値(60 mm の板で 4 mm)
const AO_RADIUS: f32 = 0.065;
const AO_STRENGTH: f32 = 1.0;
const AO_SAMPLES: f32 = 32.0;
const AO_BIAS: f32 = 0.05;
/// GPU の描画の道具一式。egui の文脈に 1 つだけ置く([`install`])。
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    layout: wgpu::BindGroupLayout,
    /// 入り隅の陰の前の段の絵と環境マップを読む束(group 1)
    ao_layout: wgpu::BindGroupLayout,
    env: EnvTextures,
    module: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    /// 入り隅の陰の前の段(面の位置を描く。作れなければ陰を描かない)
    ao_pos: Option<wgpu::RenderPipeline>,
    /// 描く管(MSAA の数ごと。既定の 4 は始めに作り、1 は初めて使う時に作る)
    pipes: Mutex<HashMap<u32, Arc<Pipes>>>,
    state: Mutex<State>,
    /// 描画装置の名前(ログと試験の表示用)
    pub adapter: String,
    /// ソフトウェアの描画装置(WARP・llvmpipe)か。描画の設定の「自動」では CPU で描く
    pub software: bool,
}

/// 1 つの MSAA の数で描く管の一揃い。
struct Pipes {
    /// 背景のグラデーション(深度を見ず書かない)
    back: wgpu::RenderPipeline,
    faces: wgpu::RenderPipeline,
    /// 断面で切っている時の切り口(裏の面だけを、切り口の色に斜めの縞を入れて塗る。M48・M11)
    caps: wgpu::RenderPipeline,
    /// 透かす道具の立体(奥行きを見ない)
    ghost_free: wgpu::RenderPipeline,
    edges: wgpu::RenderPipeline,
    /// 隠れた辺を点線で薄く描く(陰影と隠れ線。U19。手前の面より奥の所だけ)
    hidden: wgpu::RenderPipeline,
    /// 隠れた辺も描く(線だけの表示。深度を見ない)
    lines_over: wgpu::RenderPipeline,
    /// 格子と軸・透かす道具の稜線(深度を見ず、断面でも切らない)
    guides: wgpu::RenderPipeline,
    /// 本体の奥のスケッチ・作業平面(Q6 v1): 手前に見える所だけ透かして塗る面、手前の線、
    /// 隠れた所の点線(隠れた辺と同じ刻みと濃さ)
    seen_faces: wgpu::RenderPipeline,
    seen_edges: wgpu::RenderPipeline,
    seen_hidden: wgpu::RenderPipeline,
}

#[derive(Default)]
struct State {
    /// 網(場面の番号ごと)。使わなくなった場面は描くたびに捨てる
    scenes: HashMap<u64, SceneBuffers>,
    /// 描く先(ビューごと)
    targets: HashMap<egui::Id, Target>,
}

/// 場面の網を GPU に置いたもの。
struct SceneBuffers {
    /// 三角形の角ごとの位置と法線(角を共有しない)
    geometry: wgpu::Buffer,
    /// 三角形の角ごとの色(選び・マウスの下が変わると書き直す)
    colors: wgpu::Buffer,
    /// 三角形の角ごとの切り口の色(断面で裏の面を塗る色。`colors` と一緒に書き直す)
    caps: wgpu::Buffer,
    /// 角の数
    corners: u32,
    /// 色を作るのに使ったもの(同じなら書き直さない)
    color_key: u64,
    /// 辺の線分の両端
    edges: Option<wgpu::Buffer>,
    edge_vertices: u32,
    /// 辺の色(ボディごと。ボディの色が変わったら書き直す)
    edge_color: Vec<[u8; 4]>,
    /// 最後に描いたフレーム
    used: u64,
}

/// 描く先の絵。
struct Target {
    size: [u32; 2],
    samples: u32,
    /// MSAA の絵(数が 1 なら無く、`resolve` へ直に描く)
    msaa: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
    resolve: wgpu::TextureView,
    texture: egui::TextureId,
    uniforms: wgpu::Buffer,
    bind: wgpu::BindGroup,
    /// 入り隅の陰の前の段: 面の位置の絵と、その深度(MSAA なし)、読む束。陰を切っている間は
    /// 1×1 にする(4K で 1 つのビューあたり約 165 MB を、陰を使わない人も払っていた)
    ao: bool,
    ao_pos: wgpu::TextureView,
    ao_depth: wgpu::TextureView,
    ao_bind: wgpu::BindGroup,
}

/// 設計の画面の環境マップの絵(kmb_render::EnvMap::viewport を GPU へ写したもの。作るのは 1 回だけ)。
struct EnvTextures {
    /// 粗さごとの段を mip に入れた絵
    env: wgpu::TextureView,
    /// 拡散の光の絵
    irr: wgpu::TextureView,
    sampler: wgpu::Sampler,
    /// デカールの絵の層([`DECAL_LAYERS`] 枚)と、層ごとに入れた絵の key と最後に使った回
    decals: wgpu::Texture,
    decal_view: wgpu::TextureView,
    decal_keys: Mutex<DecalSlots>,
}

/// デカールの層の使い方: 使った回の数え(増えるだけ)と、層ごとの(入れた絵の key, 最後に使った回)。
type DecalSlots = (u64, Vec<Option<(u64, u64)>>);

impl EnvTextures {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> EnvTextures {
        let map = kmb_render::EnvMap::viewport();
        let upload = |label: &str, levels: &[&kmb_render::envmap::Level]| {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: levels[0].w as u32,
                    height: levels[0].h as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: levels.len() as u32,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            for (k, l) in levels.iter().enumerate() {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &tex,
                        mip_level: k as u32,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    bytemuck::cast_slice(&l.rgba16f()),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(l.w as u32 * 8),
                        rows_per_image: Some(l.h as u32),
                    },
                    wgpu::Extent3d {
                        width: l.w as u32,
                        height: l.h as u32,
                        depth_or_array_layers: 1,
                    },
                );
            }
            tex.create_view(&wgpu::TextureViewDescriptor::default())
        };
        let levels: Vec<&kmb_render::envmap::Level> = map.levels.iter().collect();
        let decals = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("kmb-view3d-decals"),
            size: wgpu::Extent3d {
                width: DECAL_SIZE,
                height: DECAL_SIZE,
                depth_or_array_layers: DECAL_LAYERS,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let decal_view = decals.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        EnvTextures {
            decals,
            decal_view,
            decal_keys: Mutex::new((0, vec![None; DECAL_LAYERS as usize])),
            env: upload("kmb-view3d-env", &levels),
            irr: upload("kmb-view3d-env-irr", &[&map.irr]),
            // 横は回り込み、縦は端で止める(EnvMap::sample と同じ)
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("kmb-view3d-env"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
        }
    }
}

/// 行列の代わりに渡す、投影の値(シェーダーの `U` と同じ並び)。
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    right: [f32; 4],
    up: [f32; 4],
    eye: [f32; 4],
    target: [f32; 4],
    /// 質感で描かない面の主の光の向き(view3d::FlatLight)と、面の中の濃淡の仮の視点までの距離(mm)
    light: [f32; 4],
    /// 入り隅の陰: 半径(mm)、強さ(0 で描かない)、標本の数、法線の向きの偏り
    ao: [f32; 4],
    /// 背景の上の端と下の端の色(sRGB の 0〜1)
    back_top: [f32; 4],
    back_low: [f32; 4],
    /// 拡大(画素/mm)、視点までの距離(平行投影は 0)、幅の半分、高さの半分(どれも point)
    proj: [f32; 4],
    /// 平行に動かした量(point)、深さの範囲の半分(mm)、近い所の限り(透視投影、mm)
    pan: [f32; 4],
    /// 遠い所の限り(mm)、辺を手前へ寄せる量(mm)、1 point の画素数、稜線の太さ(描く先の画素)
    misc: [f32; 4],
    /// 断面の平面(法線、法線の向きの位置。M48)
    section: [f32; 4],
    /// 断面で切るか(1 で切る)、隠れた辺の濃さ、点線の描く長さ・1 周期の長さ(point)
    extra: [f32; 4],
    /// 写実の見た目の環境の光(kmb_render::Env と同じ値): 光源の向きと見込む半径、輝度と cos(半径)
    lit_dir: [[f32; 4]; 3],
    lit_rad: [[f32; 4]; 3],
    /// 背景の真上・地平・地平より下の輝度(`below` の 4 つ目は光源の数)
    top: [f32; 4],
    horizon: [f32; 4],
    below: [f32; 4],
    /// 材質を描くか(1 で描く)、明るさの倍率、環境マップで描くか(1 で描く。設計の画面)
    fx: [f32; 4],
    /// ボディの材質(色の 4 つ目の値で番号を引く。[`look_slots`])
    looks: [LookU; 128],
    /// 点光源とスポットライト(数は `fx` の 4 つ目)
    lamps: [LampU; MAX_LAMPS],
}

/// 写実の見た目で照らす点光源とスポットライトの数(超えた分は描かない。光線追跡は全部)。
const MAX_LAMPS: usize = 8;

/// シェーダーへ渡す点光源・スポットライト 1 つ(kmb_render::lamp::Lamp と同じ値)。
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LampU {
    /// 位置と大きさ(半径)
    p: [f32; 4],
    /// 明るさ(線形の色)と、円すいの外の cos(点光源は -2)
    c: [f32; 4],
    /// スポットの向きと、円すいの内の cos
    d: [f32; 4],
}

/// デカールの絵を入れる層の数と、1 枚の大きさ(画素。画面の表示用に縮める)。
const DECAL_LAYERS: u32 = 8;
const DECAL_SIZE: u32 = 512;

/// 材質を持てるボディの数(色の 4 つ目の 1〜この数。超えた分は前からの描き方)。
const MAX_LOOKS: usize = 127;

/// シェーダーへ渡す材質 1 つ。
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LookU {
    /// 色(線形)と金属の度合い
    a: [f32; 4],
    /// 木目の濃い所の色(線形)と粗さ
    b: [f32; 4],
    /// 木目の芯の位置と年輪の間隔(0 なら木目なし。芯が Y の向きなら負)
    c: [f32; 4],
    /// デカール: 絵の左下の角と、絵の層の番号 + 1(0 ならデカールなし)
    d0: [f32; 4],
    /// デカール: 絵の右の向き / 幅と、不透明度
    d1: [f32; 4],
    /// デカール: 絵の上の向き / 高さと、反転の印(1 は左右、2 は上下)
    d2: [f32; 4],
}

/// 面・線・断面・道具の立体・拾う番号の絵を描く WGSL(`gpu3d.wgsl`)。
const SHADER: &str = include_str!("gpu3d.wgsl");

/// 角ごとの位置と法線。
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Corner {
    p: [f32; 3],
    n: [f32; 3],
}

/// 道具の立体 1 つの網。
struct GhostBuffers {
    geometry: wgpu::Buffer,
    colors: wgpu::Buffer,
    corners: u32,
    edges: Option<wgpu::Buffer>,
    edge_vertices: u32,
}

/// 線の端。
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LineVertex {
    p: [f32; 3],
    c: [u8; 4],
}

/// 太さを持つ線分(本体の奥のスケッチ・作業平面の縁)。
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WideSegment {
    a: [f32; 3],
    c: [u8; 4],
    b: [f32; 3],
    /// 太さ(point)
    w: f32,
}

impl Gpu {
    /// 描画装置から作る。シェーダーを作れなければ `None`(CPU の描画を使う)。
    pub fn new(rs: &egui_wgpu::RenderState) -> Option<Gpu> {
        let device = rs.device.clone();
        // 作れなかった時に落ちず、CPU の描画へ戻れるよう、誤りを受け止める範囲を開く
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("kmb-view3d"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kmb-view3d"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_entry = |binding: u32, filterable: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let ao_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kmb-view3d-ao"),
            entries: &[
                tex_entry(0, false),
                tex_entry(1, true),
                tex_entry(2, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let env = EnvTextures::new(&device, &rs.queue);
        // 番号の絵と前の段は投影の値だけ、描く管は前の段の絵も読む
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("kmb-view3d"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let main_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("kmb-view3d-main"),
            bind_group_layouts: &[Some(&layout), Some(&ao_layout)],
            immediate_size: 0,
        });
        let pipes = make_pipes(&device, &module, &main_pl, SAMPLES);
        let depth = |write: bool, compare: wgpu::CompareFunction| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            })
        };
        if let Some(e) = pollster_block(scope.pop()) {
            log::error!("GPU の描画を用意できないので、CPU で描きます: {e}");
            return None;
        }
        // 入り隅の陰の前の段(浮動小数の絵へ描けない装置では陰を描かずに続ける)
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let ao_pos = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kmb-view3d-ao-pos"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_pos_face"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Corner>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                })],
            },
            // 断面で切った時の裏の面も遮る物にするので、裏も描く(閉じたソリッドの裏は表に隠れる)
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(true, wgpu::CompareFunction::LessEqual),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_ao_pos"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: AO_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let ao_pos = match pollster_block(scope.pop()) {
            Some(e) => {
                log::warn!("入り隅の陰を描けないので、陰なしで描きます: {e}");
                None
            }
            None => Some(ao_pos),
        };
        let info = rs.adapter.get_info();
        Some(Gpu {
            device,
            queue: rs.queue.clone(),
            renderer: rs.renderer.clone(),
            layout,
            ao_layout,
            env,
            module,
            pipeline_layout: main_pl,
            ao_pos,
            pipes: Mutex::new(HashMap::from([(SAMPLES, Arc::new(pipes))])),
            state: Mutex::new(State::default()),
            adapter: format!("{} ({:?})", info.name, info.backend),
            software: info.device_type == wgpu::DeviceType::Cpu,
        })
    }

    /// MSAA の数 `samples` の管(無ければ作る。作れなければ既定の数の管)。
    fn pipes(&self, samples: u32) -> (u32, Arc<Pipes>) {
        let mut all = self.pipes.lock().expect("GPU の管");
        if let Some(p) = all.get(&samples) {
            return (samples, p.clone());
        }
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let p = make_pipes(&self.device, &self.module, &self.pipeline_layout, samples);
        if let Some(e) = pollster_block(scope.pop()) {
            log::warn!("MSAA {samples} の管を作れないので {SAMPLES} で描きます: {e}");
            return (SAMPLES, all[&SAMPLES].clone());
        }
        log::info!("3D のビューを MSAA {samples} で描く");
        let p = Arc::new(p);
        all.insert(samples, p.clone());
        (samples, p)
    }

    /// 場面を描き、egui の画像の番号を返す。`id` はビューごとに変える(描く先を使い回す)。
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        id: egui::Id,
        frame: u64,
        scene: &Scene,
        cam: &Camera,
        size: [usize; 2],
        ppp: f32,
        picked: &[Pick],
        hover: Option<&Hover>,
        colors: &Colors,
        shading: Shading,
        guides: Guides,
        ghosts: &[Ghost],
        behind: &Behind,
    ) -> egui::TextureId {
        let size = [size[0].max(1) as u32, size[1].max(1) as u32];
        let settings = crate::render::current();
        let (samples, pipes) = self.pipes(settings.samples);
        let mut state = self.state.lock().expect("GPU の描画の状態");
        let state = &mut *state;
        // 描く先: 大きさか MSAA の数か入り隅の陰の入切が変わったら作り直す(egui の画像の番号は
        // 同じものを使う)
        let want_ao = settings.ao && self.ao_pos.is_some();
        let rebuild = state
            .targets
            .get(&id)
            .is_none_or(|t| t.size != size || t.samples != samples || t.ao != want_ao);
        if rebuild {
            let old = state.targets.remove(&id).map(|t| t.texture);
            state
                .targets
                .insert(id, self.target(size, samples, want_ao, old));
        }
        let target = &state.targets[&id];
        let rect = Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(size[0] as f32 / ppp, size[1] as f32 / ppp),
        );
        let mut u = uniforms(scene, cam, rect);
        u.misc[2] = ppp;
        // 稜線の太さ: 画面の画素から描く先の画素へ(描く細かさの倍率を掛ける)
        u.misc[3] = settings.edge_px * settings.scale;
        let srgb = |c: egui::Color32| {
            let [r, g, b, _] = c.to_srgba_unmultiplied();
            [r, g, b, 255].map(|v| f32::from(v) / 255.0)
        };
        u.back_top = srgb(colors.background);
        u.back_low = srgb(colors.background_low);
        u.extra[1] = settings.hidden_alpha;
        // 入り隅の陰は面を塗る表示だけ(線だけの表示では描かない)
        let ao = settings.ao && shading.faces() && self.ao_pos.is_some();
        if ao {
            u.ao[1] = AO_STRENGTH;
        }
        self.env
            .decal_keys
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .0 += 1;
        put_looks(&mut u, scene, colors, cam.basis_pub().2, &mut |d| {
            self.decal_layer(d)
        });
        self.queue
            .write_buffer(&target.uniforms, 0, bytemuck::bytes_of(&u));

        // 網: 新しい場面なら送る。色は選びが変わった時だけ書き直す
        let key = scene.generation();
        state
            .scenes
            .entry(key)
            .or_insert_with(|| self.upload(scene));
        let sb = state.scenes.get_mut(&key).expect("今入れた");
        sb.used = frame;
        let ck = color_key(scene, picked, hover, colors, shading);
        if sb.color_key != ck && sb.corners > 0 {
            let c = face_colors(scene, picked, hover, colors, shading);
            self.queue
                .write_buffer(&sb.colors, 0, bytemuck::cast_slice(&c));
            let c = cap_colors(scene, colors);
            self.queue
                .write_buffer(&sb.caps, 0, bytemuck::cast_slice(&c));
            sb.color_key = ck;
        }
        // 稜線はボディごとの色(ボディの色から作る。CPU の描画と同じ)
        let ec: Vec<[u8; 4]> = (0..scene.bodies.len())
            .map(|bi| {
                crate::view3d::hover_edge_color(
                    hover,
                    scene.bodies[bi].feature,
                    scene.body_color(bi, colors),
                    colors,
                )
                .to_srgba_unmultiplied()
            })
            .collect();
        if sb.edge_color != ec
            && let Some(e) = &sb.edges
        {
            let v: Vec<LineVertex> = scene
                .bodies
                .iter()
                .zip(&ec)
                .flat_map(|(b, c)| b.edge_segments().iter().map(move |s| (s, *c)))
                .flat_map(|([p, q], c)| [LineVertex { p: *p, c }, LineVertex { p: *q, c }])
                .collect();
            self.queue.write_buffer(e, 0, bytemuck::cast_slice(&v));
            sb.edge_color = ec;
        }
        // 格子と軸(形より先に、深度を見ずに描く)
        let guide_lines = guide_lines(cam, rect, colors, guides, &u);
        let guide_buf = (!guide_lines.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("kmb-view3d-guides"),
                    contents: bytemuck::cast_slice(&guide_lines),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });

        // 道具の立体(層ごとの面と稜線)。小さいので毎回送る
        let ghost_bufs: Vec<(GhostLayer, GhostBuffers)> = ghosts
            .iter()
            .map(|g| (g.layer, self.ghost_buffers(g)))
            .collect();
        let draw_ghosts = |pass: &mut wgpu::RenderPass<'_>, layer: GhostLayer| {
            for (_, b) in ghost_bufs.iter().filter(|(l, _)| *l == layer) {
                if b.corners > 0 {
                    pass.set_pipeline(if layer == GhostLayer::Solid {
                        &pipes.faces
                    } else {
                        &pipes.ghost_free
                    });
                    pass.set_vertex_buffer(0, b.geometry.slice(..));
                    pass.set_vertex_buffer(1, b.colors.slice(..));
                    pass.draw(0..b.corners, 0..1);
                }
                if let Some(e) = &b.edges {
                    pass.set_vertex_buffer(0, e.slice(..));
                    if layer == GhostLayer::Solid {
                        pass.set_pipeline(&pipes.edges);
                        pass.draw(0..6, 0..b.edge_vertices / 2);
                    } else {
                        pass.set_pipeline(&pipes.guides);
                        pass.draw(0..b.edge_vertices, 0..1);
                    }
                }
            }
        };

        // 本体の奥のスケッチ・作業平面。小さいので毎回送る
        let seen_faces = (!behind.tris.is_empty()).then(|| {
            let n = |t: &[[f32; 3]; 3]| normalize3(cross3(sub3(t[1], t[0]), sub3(t[2], t[0])));
            let geometry: Vec<Corner> = behind
                .tris
                .iter()
                .flat_map(|(t, _)| t.map(|p| Corner { p, n: n(t) }))
                .collect();
            let colors: Vec<[u8; 4]> = behind
                .tris
                .iter()
                .flat_map(|(_, c)| [c.to_srgba_unmultiplied(); 3])
                .collect();
            let buf = |label: &str, contents: &[u8]| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents,
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            };
            (
                buf("kmb-view3d-seen-faces", bytemuck::cast_slice(&geometry)),
                buf("kmb-view3d-seen-colors", bytemuck::cast_slice(&colors)),
                geometry.len() as u32,
            )
        });
        let seen_lines = (!behind.lines.is_empty()).then(|| {
            let v: Vec<WideSegment> = behind
                .lines
                .iter()
                .map(|([a, b], c, w)| WideSegment {
                    a: *a,
                    c: c.to_srgba_unmultiplied(),
                    b: *b,
                    w: *w,
                })
                .collect();
            let b = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("kmb-view3d-seen-lines"),
                    contents: bytemuck::cast_slice(&v),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            (b, v.len() as u32)
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("kmb-view3d"),
            });
        // 入り隅の陰の前の段: 見えている面の位置を描く(描く管が読む)
        if let Some(ao_pos) = self.ao_pos.as_ref().filter(|_| ao && sb.corners > 0) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("kmb-view3d-ao-pos"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.ao_pos,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.ao_depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &target.bind, &[]);
            pass.set_pipeline(ao_pos);
            pass.set_vertex_buffer(0, sb.geometry.slice(..));
            pass.draw(0..sb.corners, 0..1);
        }
        {
            let [br, bg, bb, _] = colors.background.to_srgba_unmultiplied();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("kmb-view3d"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.msaa.as_ref().unwrap_or(&target.resolve),
                    depth_slice: None,
                    resolve_target: target.msaa.as_ref().map(|_| &target.resolve),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(br) / 255.0,
                            g: f64::from(bg) / 255.0,
                            b: f64::from(bb) / 255.0,
                            a: 1.0,
                        }),
                        store: if target.msaa.is_some() {
                            wgpu::StoreOp::Discard
                        } else {
                            wgpu::StoreOp::Store
                        },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &target.bind, &[]);
            pass.set_bind_group(1, &target.ao_bind, &[]);
            pass.set_pipeline(&pipes.back);
            pass.draw(0..3, 0..1);
            if let Some(b) = &guide_buf {
                pass.set_pipeline(&pipes.guides);
                pass.set_vertex_buffer(0, b.slice(..));
                pass.draw(0..guide_lines.len() as u32, 0..1);
            }
            if sb.corners > 0 {
                pass.set_pipeline(&pipes.faces);
                pass.set_vertex_buffer(0, sb.geometry.slice(..));
                pass.set_vertex_buffer(1, sb.colors.slice(..));
                pass.draw(0..sb.corners, 0..1);
                // 断面で切っている時は裏の面も描き、切り口の蓋に見せる
                if scene.section.is_some() {
                    pass.set_pipeline(&pipes.caps);
                    pass.set_vertex_buffer(1, sb.caps.slice(..));
                    pass.draw(0..sb.corners, 0..1);
                }
            }
            if shading.edges()
                && let Some(e) = &sb.edges
            {
                pass.set_vertex_buffer(0, e.slice(..));
                // 陰影と隠れ線: 面より奥の所を、先に薄い点線で描く
                if shading.hidden_dashed() {
                    pass.set_pipeline(&pipes.hidden);
                    pass.draw(0..6, 0..sb.edge_vertices / 2);
                }
                // 線だけの表示では隠れた辺も描く
                pass.set_pipeline(if shading.faces() {
                    &pipes.edges
                } else {
                    &pipes.lines_over
                });
                pass.draw(0..6, 0..sb.edge_vertices / 2);
            }
            draw_ghosts(&mut pass, GhostLayer::Solid);
            // 本体の奥のスケッチ・作業平面: 面を塗ってから、隠れた所の点線、手前の線の順に
            if let Some((geometry, colors, corners)) = &seen_faces {
                pass.set_pipeline(&pipes.seen_faces);
                pass.set_vertex_buffer(0, geometry.slice(..));
                pass.set_vertex_buffer(1, colors.slice(..));
                pass.draw(0..*corners, 0..1);
            }
            if let Some((b, n)) = &seen_lines {
                pass.set_vertex_buffer(0, b.slice(..));
                pass.set_pipeline(&pipes.seen_hidden);
                pass.draw(0..6, 0..*n);
                pass.set_pipeline(&pipes.seen_edges);
                pass.draw(0..6, 0..*n);
            }
            draw_ghosts(&mut pass, GhostLayer::Over);
        }
        self.queue.submit(Some(encoder.finish()));
        let texture = target.texture;
        // 古い場面は捨てる(形を変えるたびに番号が変わる)。途中の形と部品の両方を置けるよう数個は残す
        while state.scenes.len() > 4 {
            let oldest = state
                .scenes
                .iter()
                .min_by_key(|(_, s)| s.used)
                .map(|(k, _)| *k)
                .expect("空ではない");
            state.scenes.remove(&oldest);
        }
        texture
    }

    /// 描く先を作る。`reuse` があれば egui の画像の番号をそのまま使う。`ao` が偽なら入り隅の陰の
    /// 絵は 1×1(束ねる形は同じで、シェーダーは陰の強さ 0 で読まない)。
    fn target(
        &self,
        size: [u32; 2],
        samples: u32,
        ao: bool,
        reuse: Option<egui::TextureId>,
    ) -> Target {
        let tex_sized =
            |size: [u32; 2], label: &str, format: wgpu::TextureFormat, samples: u32, usage| {
                self.device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width: size[0],
                            height: size[1],
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage,
                        view_formats: &[],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default())
            };
        let tex = |label: &str, format: wgpu::TextureFormat, samples: u32, usage| {
            tex_sized(size, label, format, samples, usage)
        };
        let ao_size = if ao { size } else { [1, 1] };
        let msaa = (samples > 1).then(|| {
            tex(
                "kmb-view3d-msaa",
                COLOR_FORMAT,
                samples,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            )
        });
        let depth = tex(
            "kmb-view3d-depth",
            DEPTH_FORMAT,
            samples,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let resolve = tex(
            "kmb-view3d",
            COLOR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let mut r = self.renderer.write();
        let texture = match reuse {
            Some(t) => {
                r.update_egui_texture_from_wgpu_texture(
                    &self.device,
                    &resolve,
                    wgpu::FilterMode::Linear,
                    t,
                );
                t
            }
            None => r.register_native_texture(&self.device, &resolve, wgpu::FilterMode::Linear),
        };
        drop(r);
        let uniforms = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kmb-view3d-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kmb-view3d"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let ao_pos = tex_sized(
            ao_size,
            "kmb-view3d-ao-pos",
            AO_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let ao_depth = tex_sized(
            ao_size,
            "kmb-view3d-ao-depth",
            DEPTH_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let ao_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kmb-view3d-ao"),
            layout: &self.ao_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&ao_pos),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.env.env),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.env.irr),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.env.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.env.decal_view),
                },
            ],
        });
        Target {
            size,
            samples,
            msaa,
            depth,
            resolve,
            texture,
            uniforms,
            bind,
            ao,
            ao_pos,
            ao_depth,
            ao_bind,
        }
    }

    /// 道具の立体を GPU へ送る(面は三角形ごとの法線、色は透明度つき)。
    /// デカールの絵の層の番号 + 1(層が足りなければ 0 で、画面には貼らない)。初めての絵は縮めて送る。
    fn decal_layer(&self, d: &kmb_render::decal::DecalLook) -> u32 {
        let key = d.image.key;
        let mut guard = self
            .env
            .decal_keys
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (now, keys) = &mut *guard;
        let now = *now;
        if let Some(i) = keys.iter().position(|k| k.is_some_and(|k| k.0 == key)) {
            keys[i] = Some((key, now));
            return i as u32 + 1;
        }
        // 空いた層、無ければこの回に使っていないいちばん古い層
        let free = keys.iter().position(Option::is_none).or_else(|| {
            keys.iter()
                .enumerate()
                .filter(|(_, k)| k.is_some_and(|k| k.1 < now))
                .min_by_key(|(_, k)| k.map(|k| k.1))
                .map(|(i, _)| i)
        });
        let Some(i) = free else {
            log::debug!("デカールの層が足りないので、画面には貼りません(光線追跡には出る)");
            return 0;
        };
        let rgba = resample(&d.image, DECAL_SIZE);
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.env.decals,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: i as u32,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(DECAL_SIZE * 4),
                rows_per_image: Some(DECAL_SIZE),
            },
            wgpu::Extent3d {
                width: DECAL_SIZE,
                height: DECAL_SIZE,
                depth_or_array_layers: 1,
            },
        );
        keys[i] = Some((key, now));
        i as u32 + 1
    }

    fn ghost_buffers(&self, g: &Ghost) -> GhostBuffers {
        let mut corners: Vec<Corner> = Vec::with_capacity(g.tris.len() * 3);
        for t in &g.tris {
            let n = normalize3(cross3(sub3(t[1], t[0]), sub3(t[2], t[0])));
            corners.extend(t.iter().map(|p| Corner { p: *p, n }));
        }
        let c = g.color.to_srgba_unmultiplied();
        let colors = vec![c; corners.len()];
        let init = |label: &str, contents: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents,
                    usage: wgpu::BufferUsages::VERTEX,
                })
        };
        let ec = g.edge_color.to_srgba_unmultiplied();
        let edges: Vec<LineVertex> = g
            .edges
            .iter()
            .flat_map(|[p, q]| [LineVertex { p: *p, c: ec }, LineVertex { p: *q, c: ec }])
            .collect();
        GhostBuffers {
            geometry: init(
                "kmb-view3d-ghost",
                if corners.is_empty() {
                    &[0u8; std::mem::size_of::<Corner>()]
                } else {
                    bytemuck::cast_slice(&corners)
                },
            ),
            colors: init(
                "kmb-view3d-ghost-colors",
                if colors.is_empty() {
                    &[0u8; 4]
                } else {
                    bytemuck::cast_slice(&colors)
                },
            ),
            corners: corners.len() as u32,
            edges: (!edges.is_empty())
                .then(|| init("kmb-view3d-ghost-edges", bytemuck::cast_slice(&edges))),
            edge_vertices: edges.len() as u32,
        }
    }

    /// 場面の網を GPU へ送る。
    fn upload(&self, scene: &Scene) -> SceneBuffers {
        let mut corners: Vec<Corner> = Vec::new();
        let mut edges: Vec<LineVertex> = Vec::new();
        for b in &scene.bodies {
            let m = &b.mesh;
            for t in &m.triangles {
                for reverse in 0..if b.two_sided { 2 } else { 1 } {
                    let indices = if reverse == 0 { *t } else { [t[2], t[1], t[0]] };
                    for i in indices {
                        let i = i as usize;
                        corners.push(Corner {
                            p: m.positions[i],
                            n: m.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]),
                        });
                    }
                }
            }
            for [p, q] in b.edge_segments() {
                edges.push(LineVertex { p: *p, c: [0; 4] });
                edges.push(LineVertex { p: *q, c: [0; 4] });
            }
        }
        let n = corners.len().max(1);
        let geometry = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("kmb-view3d-geometry"),
                contents: if corners.is_empty() {
                    &[0u8; std::mem::size_of::<Corner>()]
                } else {
                    bytemuck::cast_slice(&corners)
                },
                usage: wgpu::BufferUsages::VERTEX,
            });
        let colors = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kmb-view3d-colors"),
            size: (n * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let caps = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kmb-view3d-caps"),
            size: (n * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let edge_buf = (!edges.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("kmb-view3d-edges"),
                    contents: bytemuck::cast_slice(&edges),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                })
        });
        SceneBuffers {
            geometry,
            colors,
            caps,
            corners: corners.len() as u32,
            color_key: u64::MAX,
            edges: edge_buf,
            edge_vertices: edges.len() as u32,
            edge_color: Vec::new(),
            used: 0,
        }
    }
}

/// MSAA の数 `samples` で描く管を作る。
fn make_pipes(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    pl: &wgpu::PipelineLayout,
    samples: u32,
) -> Pipes {
    let corner = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Corner>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
    };
    let color = wgpu::VertexBufferLayout {
        array_stride: 4,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![2 => Unorm8x4],
    };
    let line = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<LineVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4],
    };
    // 稜線の四角: 線の 2 頂点を 1 つの組として、組ごとに 6 頂点を描く
    let wide = wgpu::VertexBufferLayout {
        array_stride: 2 * std::mem::size_of::<LineVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x3,
            1 => Unorm8x4,
            4 => Float32x3,
        ],
    };
    let wide_w = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<WideSegment>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x3,
            1 => Unorm8x4,
            4 => Float32x3,
            5 => Float32,
        ],
    };
    let target = [Some(wgpu::ColorTargetState {
        format: COLOR_FORMAT,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let depth = |write: bool, compare: wgpu::CompareFunction| {
        Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(write),
            depth_compare: Some(compare),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        })
    };
    let pipeline = |label: &str,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    topology: wgpu::PrimitiveTopology,
                    cull: Option<wgpu::Face>,
                    depth_stencil: Option<wgpu::DepthStencilState>| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(pl),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some(vs),
                compilation_options: Default::default(),
                buffers,
            },
            primitive: wgpu::PrimitiveState {
                topology,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: cull,
                ..Default::default()
            },
            depth_stencil,
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some(fs),
                compilation_options: Default::default(),
                targets: &target,
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let face_buffers = [Some(corner), Some(color)];
    let line_buffers = [Some(line)];
    let wide_buffers = [Some(wide)];
    let wide_w_buffers = [Some(wide_w)];
    let tri = wgpu::PrimitiveTopology::TriangleList;
    let lines = wgpu::PrimitiveTopology::LineList;
    Pipes {
        back: pipeline(
            "kmb-view3d-back",
            "vs_back",
            "fs_back",
            &[],
            tri,
            None,
            depth(false, wgpu::CompareFunction::Always),
        ),
        // 閉じたソリッドなので裏向きの三角形は見えない(CPU の描画と同じ)
        faces: pipeline(
            "kmb-view3d-faces",
            "vs_face",
            "fs_face",
            &face_buffers,
            tri,
            Some(wgpu::Face::Back),
            depth(true, wgpu::CompareFunction::LessEqual),
        ),
        caps: pipeline(
            "kmb-view3d-caps",
            "vs_cap",
            "fs_cap",
            &face_buffers,
            tri,
            Some(wgpu::Face::Front),
            depth(true, wgpu::CompareFunction::LessEqual),
        ),
        // 透かす道具の立体: 奥行きを見ず書かず、裏も描く
        ghost_free: pipeline(
            "kmb-view3d-ghost",
            "vs_face",
            "fs_ghost",
            &face_buffers,
            tri,
            None,
            depth(false, wgpu::CompareFunction::Always),
        ),
        edges: pipeline(
            "kmb-view3d-edges",
            "vs_wide",
            "fs_wide",
            &wide_buffers,
            tri,
            None,
            depth(false, wgpu::CompareFunction::LessEqual),
        ),
        hidden: pipeline(
            "kmb-view3d-hidden",
            "vs_wide",
            "fs_wide_hidden",
            &wide_buffers,
            tri,
            None,
            depth(false, wgpu::CompareFunction::Greater),
        ),
        lines_over: pipeline(
            "kmb-view3d-lines-over",
            "vs_wide",
            "fs_wide",
            &wide_buffers,
            tri,
            None,
            depth(false, wgpu::CompareFunction::Always),
        ),
        guides: pipeline(
            "kmb-view3d-guides",
            "vs_line",
            "fs_guide",
            &line_buffers,
            lines,
            None,
            depth(false, wgpu::CompareFunction::Always),
        ),
        seen_faces: pipeline(
            "kmb-view3d-seen-faces",
            "vs_seen_face",
            "fs_flat",
            &face_buffers,
            tri,
            None,
            depth(false, wgpu::CompareFunction::LessEqual),
        ),
        seen_edges: pipeline(
            "kmb-view3d-seen-edges",
            "vs_wide_w",
            "fs_wide",
            &wide_w_buffers,
            tri,
            None,
            depth(false, wgpu::CompareFunction::LessEqual),
        ),
        seen_hidden: pipeline(
            "kmb-view3d-seen-hidden",
            "vs_wide_w",
            "fs_wide_hidden",
            &wide_w_buffers,
            tri,
            None,
            depth(false, wgpu::CompareFunction::Greater),
        ),
    }
}

/// 待つだけの小さな実行器(エラーの範囲を閉じる時だけ使う。native の wgpu はすぐ答える)。
pub fn pollster_block<F: std::future::Future>(f: F) -> F::Output {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    let mut f = std::pin::pin!(f);
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

/// 投影の値。CPU の [`Camera::project`] と同じ写し方になるようにそろえる。
fn uniforms(scene: &Scene, cam: &Camera, rect: Rect) -> Uniforms {
    let (r, u, e) = cam.basis_pub();
    // 深さの範囲: 回す中心から場面の一番遠い角まで(格子と軸は深度を書かないので入れない)
    let mut reach = 1.0f32;
    if !scene.is_empty() {
        let (lo, hi) = scene.bounds();
        for i in 0..8 {
            let c = [
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ];
            let d = [0, 1, 2].map(|k| c[k] - cam.target[k]);
            reach = reach.max((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt());
        }
    }
    let size = {
        let (lo, hi) = scene.bounds();
        let s = [0, 1, 2].map(|k| (hi[k] - lo[k]).max(0.0));
        (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt()
    };
    let size = if size.is_finite() { size } else { 0.0 };
    let bias = 0.003 * size;
    // 質感で描かない面の光(CPU の描画と同じ。場面の半径は対角の半分)
    let flat = crate::view3d::FlatLight::new(e, cam.target, (0.5 * size).max(1.0));
    let reach = reach * 1.05 + bias;
    let dist = cam.distance(rect).unwrap_or(0.0);
    let near = dist * crate::view3d::NEAR;
    // 遠い所の限りは格子の端も入るように広く取る(深さの精度は 32 ビットの浮動小数で足りる)
    let far = (dist + reach).max(dist * 100.0);
    let v4 = |a: [f32; 3]| [a[0], a[1], a[2], 0.0];
    Uniforms {
        right: v4(r),
        up: v4(u),
        eye: v4(e),
        target: v4(cam.target),
        light: [flat.key[0], flat.key[1], flat.key[2], flat.dist],
        // 強さは描く時に設定から入れる
        ao: [AO_RADIUS * size, 0.0, AO_SAMPLES, AO_BIAS],
        back_top: [0.0; 4],
        back_low: [0.0; 4],
        proj: [cam.zoom, dist, rect.width() / 2.0, rect.height() / 2.0],
        pan: [cam.pan.x, cam.pan.y, reach, near],
        misc: [far.max(near + 1.0), bias, 0.0, 0.0],
        section: match scene.section {
            Some(sec) => [sec.normal[0], sec.normal[1], sec.normal[2], sec.offset],
            None => [0.0; 4],
        },
        // 隠れた辺の点線: 4 point 描いて 3 point あける(CPU の描画と同じ刻み)
        extra: [
            if scene.section.is_some() { 1.0 } else { 0.0 },
            1.0,
            crate::view3d::DASH_ON,
            crate::view3d::DASH_ON + crate::view3d::DASH_OFF,
        ],
        lit_dir: [[0.0; 4]; 3],
        lit_rad: [[0.0; 4]; 3],
        top: [0.0; 4],
        horizon: [0.0; 4],
        below: [0.0; 4],
        fx: [0.0; 4],
        looks: [<LookU as bytemuck::Zeroable>::zeroed(); 128],
        lamps: [<LampU as bytemuck::Zeroable>::zeroed(); MAX_LAMPS],
    }
}

/// 写実の見た目の環境の光と、ボディの材質を入れる。
fn put_looks(
    u: &mut Uniforms,
    scene: &Scene,
    colors: &Colors,
    toward_eye: [f32; 3],
    decal_layer: &mut dyn FnMut(&kmb_render::decal::DecalLook) -> u32,
) {
    let env = kmb_render::Env::new(scene.effects.env, toward_eye);
    let lin = |c: [u8; 3]| c.map(kmb_render::math::srgb_to_linear);
    for (k, l) in env.lights.iter().take(3).enumerate() {
        u.lit_dir[k] = [l.dir[0], l.dir[1], l.dir[2], l.half];
        u.lit_rad[k] = [l.radiance[0], l.radiance[1], l.radiance[2], l.cos_half];
    }
    let v4 = |a: [f32; 3], w: f32| [a[0], a[1], a[2], w];
    u.top = v4(env.backdrop([0.0, 0.0, 1.0]), 0.0);
    u.horizon = v4(env.backdrop([1.0, 0.0, 0.0]), 0.0);
    u.below = v4(
        env.backdrop([0.0, 0.0, -1.0]),
        env.lights.len().min(3) as f32,
    );
    let slots = look_slots(scene, colors);
    // 点光源とスポットライトはレンダリングの作業空間(写実の環境の光で描く時)だけ照らす
    let lamps: &[kmb_render::lamp::Lamp] = if scene.effects.all_bodies {
        scene.lamps()
    } else {
        &[]
    };
    for (k, l) in lamps.iter().take(MAX_LAMPS).enumerate() {
        u.lamps[k] = LampU {
            p: v4(l.pos, l.radius),
            c: v4(l.power, l.cos_outer),
            d: v4(l.dir, l.cos_inner),
        };
    }
    u.fx = [
        if !slots.looks.is_empty() { 1.0 } else { 0.0 },
        env.exposure,
        // 設計の画面(レンダリングの作業空間でない)は環境マップで描く
        if scene.effects.all_bodies { 0.0 } else { 1.0 },
        lamps.len().min(MAX_LAMPS) as f32,
    ];
    for (slot, look) in slots.looks {
        let g = look.finish.grain;
        let ring = match g {
            Some(g) if look.frame.axis == 1 => -g.ring.max(0.1),
            Some(g) => g.ring.max(0.1),
            None => 0.0,
        };
        let (d0, d1, d2) = match look.decal.as_deref() {
            Some(d) if d.frame.is_some() => {
                let f = d.frame.expect("今見た");
                let layer = decal_layer(d);
                let flags = (d.place.flip_h as u8 + 2 * d.place.flip_v as u8) as f32;
                (
                    v4(f.origin, layer as f32),
                    v4(f.u, d.place.opacity as f32),
                    v4(f.v, flags),
                )
            }
            _ => ([0.0; 4], [0.0; 4], [0.0; 4]),
        };
        u.looks[slot as usize - 1] = LookU {
            a: v4(lin(look.color), look.metallic()),
            b: v4(lin(g.map_or(look.color, |g| g.dark)), look.roughness()),
            c: v4(look.frame.center, ring),
            d0,
            d1,
            d2,
        };
    }
}

/// 材質の番号(1〜[`MAX_LOOKS`])。
struct Slots {
    /// 表示しているボディごとの番号(写実で描かないボディは `None`)
    bodies: Vec<Option<u8>>,
    /// 表示しているボディの面ごとの、面の外観の番号(`mesh.face_names` の順。面の外観の無い面は `None`)
    faces: Vec<Vec<Option<u8>>>,
    /// 番号と材質
    looks: Vec<(u8, kmb_render::Look)>,
}

/// 表示しているボディと面の外観の材質に番号を振る。番号が足りなくなった物は写実で描かない。
fn look_slots(scene: &Scene, colors: &Colors) -> Slots {
    let mut looks: Vec<(u8, kmb_render::Look)> = Vec::new();
    let mut add = |look: kmb_render::Look| {
        if looks.len() >= MAX_LOOKS {
            return None;
        }
        let slot = (looks.len() + 1) as u8;
        looks.push((slot, look));
        Some(slot)
    };
    let bodies: Vec<Option<u8>> = (0..scene.bodies.len())
        .map(|bi| add(scene.body_look(bi, colors)?))
        .collect();
    let faces = scene
        .bodies
        .iter()
        .enumerate()
        .map(|(bi, b)| {
            (0..b.face_looks.len())
                .map(|k| add(scene.face_look(bi, k)?.clone()))
                .collect()
        })
        .collect();
    Slots {
        bodies,
        faces,
        looks,
    }
}

/// 絵を `size` × `size` 画素に描き直す(隣の 4 画素を混ぜる。縦横の比はシェーダーが戻す)。
fn resample(im: &kmb_render::decal::DecalImage, size: u32) -> Vec<u8> {
    let (w, h) = (im.width.max(1), im.height.max(1));
    let mut out = vec![0u8; (size * size * 4) as usize];
    if im.rgba.len() < (w * h * 4) as usize {
        return out;
    }
    for y in 0..size {
        let fy = ((y as f32 + 0.5) / size as f32 * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (y0, ty) = (fy.floor() as u32, fy.fract());
        let y1 = (y0 + 1).min(h - 1);
        for x in 0..size {
            let fx = ((x as f32 + 0.5) / size as f32 * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
            let (x0, tx) = (fx.floor() as u32, fx.fract());
            let x1 = (x0 + 1).min(w - 1);
            let px = |x: u32, y: u32, c: usize| im.rgba[((y * w + x) * 4) as usize + c] as f32;
            for c in 0..4 {
                let top = px(x0, y0, c) * (1.0 - tx) + px(x1, y0, c) * tx;
                let low = px(x0, y1, c) * (1.0 - tx) + px(x1, y1, c) * tx;
                out[((y * size + x) * 4) as usize + c] =
                    (top * (1.0 - ty) + low * ty).round() as u8;
            }
        }
    }
    out
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize3(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l > 0.0 { a.map(|v| v / l) } else { a }
}

/// 面の色を作るのに使うものの指紋。
fn color_key(
    scene: &Scene,
    picked: &[Pick],
    hover: Option<&Hover>,
    colors: &Colors,
    shading: Shading,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for b in &scene.bodies {
        b.tint.map(|c| c.to_array()).hash(&mut h);
    }
    scene.faded().hash(&mut h);
    scene
        .glow()
        .map(|(s, c, k)| (s, c.to_array(), k))
        .hash(&mut h);
    // 切り口の色(断面の塗り分けと、全部のボディで同じ色の時の色)
    scene.section.map(|s| (s.by_body, s.outside)).hash(&mut h);
    crate::view3d::cap_color(colors).to_array().hash(&mut h);
    // ボディの既定の色(稜線の色で明るさの振り方が変わる)
    (colors.body.to_array(), colors.edge.to_array()).hash(&mut h);
    for p in picked {
        (p.feature, &p.face).hash(&mut h);
    }
    hover.hash(&mut h);
    [
        colors.body,
        colors.picked,
        colors.hover,
        colors.edge,
        colors.line,
    ]
    .map(|c| c.to_array())
    .hash(&mut h);
    shading.hash(&mut h);
    scene.effects.hash(&mut h);
    for b in &scene.bodies {
        b.look.as_ref().map(|l| l.color).hash(&mut h);
        // 面の外観は色の 4 つ目(材質の番号)も替わるので、材質のあるなしと色を入れる
        for l in &b.face_looks {
            l.as_ref().map(|l| l.color).hash(&mut h);
        }
    }
    h.finish()
}

/// 三角形の角ごとの色(sRGB)。塗らない面は透明(シェーダーで捨てる)。
fn face_colors(
    scene: &Scene,
    picked: &[Pick],
    hover: Option<&Hover>,
    colors: &Colors,
    shading: Shading,
) -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    let slots = look_slots(scene, colors);
    for (bi, b) in scene.bodies.iter().enumerate() {
        let m = &b.mesh;
        let body = scene.body_color(bi, colors);
        // 材質で描く面は、色の 4 つ目を材質の番号にする(選んだ面・マウスの下の面は前からの色)
        let slot = slots
            .bodies
            .get(bi)
            .copied()
            .flatten()
            .filter(|_| shading.faces() && !scene.glows(bi))
            .unwrap_or(if scene.is_faded(bi) {
                254
            } else if shading.faces() && scene.steel_env(bi) {
                253
            } else {
                255
            });
        // 面ごとに一度だけ決める
        let per_face: Vec<[u8; 4]> = m
            .face_names
            .iter()
            .enumerate()
            .map(|(k, name)| {
                let is = |p: &Pick| p.feature == b.feature && p.face == *name;
                let c = if picked.iter().any(is) {
                    colors.picked
                } else if hover.is_some_and(|h| h.matches_face(b.feature, name)) {
                    colors.hover
                } else if shading.faces() {
                    match b.colors.get(k).copied().flatten() {
                        Some(c) => scene.glow_tint(bi, c),
                        None => {
                            // 面の外観のある面は、その色と材質の番号(番号が無ければ前からの塗り)
                            if let Some(c) = scene.face_tint(bi, k, colors) {
                                let face_slot = slots
                                    .faces
                                    .get(bi)
                                    .and_then(|f| f.get(k))
                                    .copied()
                                    .flatten()
                                    .filter(|_| shading.faces());
                                let [r, g, bb, _] = c.to_srgba_unmultiplied();
                                return [
                                    r,
                                    g,
                                    bb,
                                    face_slot.unwrap_or(if slot == 254 { 254 } else { 255 }),
                                ];
                            }
                            let [r, g, bb, _] = body.to_srgba_unmultiplied();
                            return [r, g, bb, slot];
                        }
                    }
                } else {
                    return [0; 4];
                };
                let [r, g, bb, _] = c.to_srgba_unmultiplied();
                [r, g, bb, 255]
            })
            .collect();
        for f in &m.triangle_faces {
            let c = per_face.get(*f as usize).copied().unwrap_or([0; 4]);
            out.extend(std::iter::repeat_n(c, if b.two_sided { 6 } else { 3 }));
        }
    }
    out
}

/// 三角形の角ごとの切り口の色(sRGB。ボディごとに [`Scene::cap_color`])。
fn cap_colors(scene: &Scene, colors: &Colors) -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for (bi, b) in scene.bodies.iter().enumerate() {
        let [r, g, bb, _] = scene.cap_color(bi, colors).to_srgba_unmultiplied();
        out.extend(std::iter::repeat_n(
            [r, g, bb, 255],
            b.mesh.triangles.len() * if b.two_sided { 6 } else { 3 },
        ));
    }
    out
}

/// 格子と軸の線分(CPU の描画の `draw_guides` と同じ線)。見える範囲で切ってから渡す
/// (画面の外へ遠く伸びる線や視点の後ろへ伸びる線をそのまま渡すと、ソフトウェアの描画装置で
/// 線の代わりに白い塊が出た)。
fn guide_lines(
    cam: &Camera,
    rect: Rect,
    colors: &Colors,
    g: Guides,
    u: &Uniforms,
) -> Vec<LineVertex> {
    let mut out = Vec::new();
    crate::view3d::guide_segments(cam, rect, colors, g, &mut |a, b, c| {
        let Some((a, b)) = clip_line(u, a, b) else {
            return;
        };
        let c = c.to_srgba_unmultiplied();
        out.push(LineVertex { p: a, c });
        out.push(LineVertex { p: b, c });
    });
    out
}

/// シェーダーの `clip` と同じ写し方(線の手前へ寄せる量は入れない)。
fn clip_point(u: &Uniforms, p: [f32; 3]) -> [f32; 4] {
    let d = [0, 1, 2].map(|i| p[i] - u.target[i]);
    let dot = |v: [f32; 4]| d[0] * v[0] + d[1] * v[1] + d[2] * v[2];
    let (x, y, z) = (dot(u.right), dot(u.up), dot(u.eye));
    let [zoom, dist, hw, hh] = u.proj;
    if dist <= 0.0 {
        let depth = ((u.pan[2] - z) / (2.0 * u.pan[2])).clamp(0.0, 1.0);
        return [
            (x * zoom + u.pan[0]) / hw,
            (y * zoom - u.pan[1]) / hh,
            depth,
            1.0,
        ];
    }
    let t = dist - z;
    let k = zoom * dist;
    let (near, far) = (u.pan[3], u.misc[0]);
    [
        (x * k + u.pan[0] * t) / hw,
        (y * k - u.pan[1] * t) / hh,
        far / (far - near) * (t - near),
        t,
    ]
}

/// 線分を見える範囲(少し広め)で切る。写し方は同次座標で線形なので、空間の点も同じ割合で切れる。
fn clip_line(u: &Uniforms, a: [f32; 3], b: [f32; 3]) -> Option<([f32; 3], [f32; 3])> {
    let (ca, cb) = (clip_point(u, a), clip_point(u, b));
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    // 各面で「内側なら正」になる値
    const M: f32 = 1.1;
    let planes: [fn([f32; 4]) -> f32; 5] = [
        |c| M * c[3] - c[0],
        |c| M * c[3] + c[0],
        |c| M * c[3] - c[1],
        |c| M * c[3] + c[1],
        |c| c[2],
    ];
    for f in planes {
        let (da, db) = (f(ca), f(cb));
        if da < 0.0 && db < 0.0 {
            return None;
        }
        if da < 0.0 {
            t0 = t0.max(da / (da - db));
        } else if db < 0.0 {
            t1 = t1.min(da / (da - db));
        }
    }
    if t0 >= t1 {
        return None;
    }
    let at = |t: f32| [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t);
    Some((at(t0), at(t1)))
}

/// egui の文脈に GPU の描画を置く(アプリの起動時に 1 度)。`force` なら、ソフトウェアの描画装置でも
/// 描画の設定の「自動」で GPU の描き方を使う(`JC_VIEW3D=gpu`。クラウドでの点検用)。
pub fn install(ctx: &egui::Context, rs: &egui_wgpu::RenderState, force: bool) -> bool {
    match make(rs, force, SAMPLES) {
        Some(g) => {
            put(ctx, g);
            true
        }
        None => false,
    }
}

/// [`install`] を別のスレッドで始める。シェーダーを作る間(DirectX 12 の DXC で 0.1 秒ほど)に、
/// 起動の残り(フォント・文書・最初の画面の組み立て)を進める。初めて [`get`] を呼んだ時に
/// まだ作っていれば、作り終えるまで待つ。`samples` は描画の設定の MSAA の数(既定の 4 と
/// 違えば、その数の管も先に作る)。
pub fn install_in_background(
    ctx: &egui::Context,
    rs: &egui_wgpu::RenderState,
    force: bool,
    samples: u32,
) {
    let rs = rs.clone();
    let spawned = std::thread::Builder::new()
        .name("kmb-gpu3d".into())
        .spawn(move || make(&rs, force, samples));
    match spawned {
        Ok(handle) => {
            let pending = Arc::new(Pending(Mutex::new(Some(handle))));
            ctx.data_mut(|d| d.insert_temp(egui::Id::new(PENDING), pending));
        }
        Err(e) => log::warn!("3D のビューの用意を別のスレッドで始められません: {e}"),
    }
}

/// 別のスレッドで作っている GPU の描画。
struct Pending(Mutex<Option<std::thread::JoinHandle<Option<Gpu>>>>);

/// GPU の描画を作る(作れなければ `None`)。
fn make(rs: &egui_wgpu::RenderState, force: bool, samples: u32) -> Option<Gpu> {
    let t = std::time::Instant::now();
    let mut g = Gpu::new(rs)?;
    g.software &= !force;
    if samples != SAMPLES {
        g.pipes(samples);
    }
    log::info!(
        "3D のビューを GPU で描く: {}(シェーダーと管を作るのに {:.0} ms)",
        g.adapter,
        t.elapsed().as_secs_f64() * 1e3
    );
    Some(g)
}

fn put(ctx: &egui::Context, g: Gpu) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(KEY), Arc::new(g)));
}

/// 別のスレッドで作っていれば、作り終えるまで待って置く。
fn finish_pending(ctx: &egui::Context) {
    let Some(pending) = ctx.data(|d| d.get_temp::<Arc<Pending>>(egui::Id::new(PENDING))) else {
        return;
    };
    let handle = pending.0.lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some(handle) = handle else {
        return;
    };
    let t = std::time::Instant::now();
    let made = handle.join().unwrap_or_else(|_| {
        log::warn!("3D のビューの用意が止まりました。CPU で描きます");
        None
    });
    log::info!(
        "3D のビューの用意を待った: {:.0} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
    if let Some(g) = made {
        put(ctx, g);
    }
    ctx.data_mut(|d| d.remove::<Arc<Pending>>(egui::Id::new(PENDING)));
}

/// 置いてある GPU の描画。無いか、描画の設定で CPU を選んでいれば `None`(CPU で描く)。
/// 「自動」では、ソフトウェアの描画装置なら CPU で描く(CPU の描画の方が速い。2026-10-16
/// クラウドの llvmpipe で 1600×1000、等角で回して 27 fps 対 19 fps)。
pub fn get(ctx: &egui::Context) -> Option<Arc<Gpu>> {
    finish_pending(ctx);
    let g = ctx.data(|d| d.get_temp::<Arc<Gpu>>(egui::Id::new(KEY)))?;
    let mode = ctx
        .data(|d| d.get_temp::<crate::render::View3dMode>(egui::Id::new(MODE)))
        .unwrap_or_default();
    match mode {
        crate::render::View3dMode::Gpu => Some(g),
        crate::render::View3dMode::Cpu => None,
        crate::render::View3dMode::Auto => (!g.software).then_some(g),
    }
}

/// 3D のビューを描く所(描画の設定。すぐ替わる)。
pub fn set_mode(ctx: &egui::Context, mode: crate::render::View3dMode) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(MODE), mode));
}

const KEY: &str = "kmb-ui::gpu3d";
const PENDING: &str = "kmb-ui::gpu3d::pending";
const MODE: &str = "kmb-ui::gpu3d::mode";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_hover_colors_all_its_faces_and_edges_without_changing_other_bodies() {
        use egui::Color32;
        use kmb_core::{FeatureId, Mesh};
        let mesh = Mesh {
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            normals: vec![[0., 0., 1.]; 4],
            triangles: vec![[0, 1, 2], [0, 3, 1]],
            triangle_faces: vec![0, 1],
            face_names: vec!["top".into(), "front".into()],
            edges: vec![],
        };
        let mut scene = Scene::default();
        let (a, b) = (FeatureId(1), FeatureId(2));
        scene.set(&[], vec![(a, mesh.clone()), (b, mesh)]);
        let colors = Colors::from_look(crate::view3d::Palette {
            dark: true,
            canvas: Color32::BLACK,
            ink: Color32::WHITE,
            ink2: Color32::WHITE,
            ink3: Color32::GRAY,
            err: Color32::RED,
            warn: Color32::YELLOW,
            design: Color32::GREEN,
            cam: Color32::GREEN,
            shade3: Color32::GRAY,
            accent: Color32::BLUE,
            inks: crate::Inks::DARK,
        });
        let plain = face_colors(&scene, &[], None, &colors, Shading::ShadedEdges);
        let body = Hover::Body(a);
        let all = face_colors(&scene, &[], Some(&body), &colors, Shading::ShadedEdges);
        let hover = colors.hover.to_srgba_unmultiplied();
        assert!(all[..6].iter().all(|c| *c == hover));
        assert_eq!(all[6..], plain[6..], "別のボディは変えない");
        let face = Pick {
            feature: a,
            face: "top".into(),
        };
        let single = Hover::Face(face.clone());
        let one = face_colors(&scene, &[], Some(&single), &colors, Shading::ShadedEdges);
        assert!(one[..3].iter().all(|c| *c == hover));
        assert_eq!(one[3..], plain[3..], "面のホバーは1面だけ");
        let selected = face_colors(&scene, &[face], Some(&body), &colors, Shading::ShadedEdges);
        assert!(
            selected[..3]
                .iter()
                .all(|c| *c == colors.picked.to_srgba_unmultiplied())
        );
        assert!(selected[3..6].iter().all(|c| *c == hover));
        let edge = |hover, id, bi| {
            crate::view3d::hover_edge_color(hover, id, scene.body_color(bi, &colors), &colors)
        };
        assert_ne!(edge(Some(&body), a, 0), edge(None, a, 0));
        assert_eq!(edge(Some(&body), b, 1), edge(None, b, 1));
        assert_eq!(edge(Some(&single), a, 0), edge(None, a, 0));
        assert_ne!(
            color_key(&scene, &[], Some(&body), &colors, Shading::ShadedEdges),
            color_key(&scene, &[], Some(&single), &colors, Shading::ShadedEdges)
        );
    }

    /// GPU の写し方が CPU の [`Camera::project`] と同じ画面の位置になる(平行・透視)。
    #[test]
    fn the_gpu_projection_lands_where_the_cpu_one_does() {
        let scene = Scene::default();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        for perspective in [false, true] {
            let mut cam = Camera::default();
            cam.perspective = perspective;
            cam.fit_box([-30.0, -20.0, 0.0], [30.0, 20.0, 15.0], rect);
            cam.pan = egui::vec2(25.0, -10.0);
            let u = uniforms(&scene, &cam, rect);
            for p in [[0.0, 0.0, 0.0], [30.0, -20.0, 15.0], [-12.5, 7.0, 3.0]] {
                let c = clip_point(&u, p);
                let (ndc_x, ndc_y) = (c[0] / c[3], c[1] / c[3]);
                let gpu = egui::pos2(
                    rect.center().x + ndc_x * rect.width() / 2.0,
                    rect.center().y - ndc_y * rect.height() / 2.0,
                );
                let cpu = cam.project(p, rect).0;
                assert!(
                    (gpu - cpu).length() < 0.01,
                    "透視 {perspective}: {p:?} → GPU {gpu:?}、CPU {cpu:?}"
                );
                let z = c[2] / c[3];
                assert!((0.0..=1.0).contains(&z), "深さ {z} が範囲の外");
            }
        }
    }

    /// 視点の後ろへ伸びる線と画面の外へ遠く伸びる線は、見える範囲で切れる。
    #[test]
    fn long_lines_are_cut_to_what_can_be_seen() {
        let scene = Scene::default();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut cam = Camera::default();
        cam.perspective = true;
        cam.fit_box([-30.0, -20.0, 0.0], [30.0, 20.0, 15.0], rect);
        let u = uniforms(&scene, &cam, rect);
        let eye = cam.eye_position(rect).unwrap();
        // 視点を通り越して後ろへ伸びる線
        let behind = [0, 1, 2].map(|i| eye[i] + (eye[i] - cam.target[i]) * 3.0);
        let (a, b) = clip_line(&u, cam.target, behind).expect("手前の部分は見える");
        for q in [a, b] {
            let c = clip_point(&u, q);
            assert!(c[3] > 0.0 && c[2] >= -1e-4, "{q:?} は視点の前: {c:?}");
            assert!(c[0].abs() <= 1.1 * c[3] + 1e-3 && c[1].abs() <= 1.1 * c[3] + 1e-3);
        }
        // 全部が画面の外の線は捨てる
        assert!(clip_line(&u, [1e6, 1e6, 0.0], [1e6 + 1.0, 1e6, 0.0]).is_none());
    }
}
