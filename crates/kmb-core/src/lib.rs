//! kieMonoBench のコア。
//!
//! 文書モデルは純データ(パラメータ、機能の種類とデータ、ロールバック位置)で、
//! 形状などの出力は再計算で作り直す派生キャッシュである。取り消しは文書モデルへの
//! 変更を戻すだけで済む。CAD の操作も CAM の操作も同じ [`Feature`] として扱う。

pub mod args;
mod bodies;
mod descriptor;
mod document;
mod extension;
mod face_names;
mod feature;
mod ids;
pub mod kernel;
mod modules;
mod names;
mod occurrence;
mod payload;
mod problem;
mod recompute;
mod refs;
pub mod section;
mod transaction;

pub use args::{Arg, ArgKind};
pub use bodies::{BodyId, BodyLine, BodyState, BodyTable};
pub use descriptor::{
    CHOICES_TYPE, Choice, ChoiceIllustration, ChoiceLines, Descriptor, Field, FieldKind, FieldUnit,
    Fix, PickOn, Retarget, Slider, Step, Suggest, When, category, choices_of, choices_value,
    row_name,
};
pub use document::{
    Component, DOCUMENT_EXTENSION, DOCUMENT_EXTENSIONS, DocEvent, Document, DocumentData,
    FORMAT_VERSION, FeatureEntry, Group, LoadError, NamedView, Param, ParamKey, ParamUnit,
    ParamView, Properties,
};
pub use extension::{
    Action, ActionRole, BoxInk, BoxKind, Context, Dof, Drag, DragMotion, EdgePick, Edit, Extension,
    Extensions, FacePick, FileGate, Frame, FrameBar, Gesture, Handle, HandleChoice, Input, Job,
    JobReport, KindRole, LineInk, LineKind, LinkInfo, LinkStatus, MarkKind, Outcome, Overlay,
    OverlayPicture, OverlayPlane, OverlayShape, Place, PlaneInk, Playback, RunState, RunTone,
    Series, Settings, SettingsRole, ShapeInk, Stage, StageBand, StepMark, Table, TableColumn,
    TableMark, TableOption, sibling_path,
};
pub use extension::{JobControl, JobProgress};
pub use face_names::{FaceAliases, SPLIT_BY};
pub use feature::{
    CONCURRENT, EvalCtx, EvalError, Factory, Feature, InputRef, ModelParam, Names, Outputs,
    Registry, Value, WARNINGS, add_warnings, expr_inputs, template_inputs, warnings_of,
};
pub use ids::{FeatureId, ParamId};
pub use kernel::{
    ArcEdge, BlendLaw, BooleanOp, CURVES_TYPE, Curves, EdgeBlend, EdgeInfo, FaceInfo, Kernel,
    KernelError, MassProps, Mesh, NullKernel, PROFILE_FACES_TYPE, PROFILE_TYPE, PlanarFace,
    PlanarPath, Plane, Profile, ProfileEdge, ProfileFace, ProfileFaces, ProfileLoop, Region, Seam,
    Segment, ShapeHandle, canonical_loop, merge_regions, natural_key, point_in_polygon,
};
pub use modules::{Module, Modules, registry_of};
pub use names::unique_name;
pub use payload::{
    LINES_TYPE, Lines, PICTURE, POINTS_TYPE, Payload, Picture, PointId, Points, TOOL_PREVIEW,
    ToolOp, ToolPreview, WIRES_TYPE, Warnings, Wire, Wires,
};
pub use problem::{Problem, ProblemKind};
pub use recompute::{
    Cache, FeatureResult, FeatureStatus, MOVED, ModelParamResult, NESTED_DEPTH, ParamResult,
    ParamStatus, RecomputeReport, body_heir, body_labels, body_root, eval_exprs, model_params,
    moved_slot, reads_now, recompute_cancellable, recompute_detached, recompute_nested,
    recompute_watched, visible_bodies, visible_bodies_before,
};
pub use refs::{
    Ref, RefKind, VertexInfo, VertexPick, face_feature, one_or_many, vertices, vertices_of_edges,
};
pub use section::{Section, SectionError, SectionType};
pub use transaction::{Op, Transaction, TxError};

pub use kmb_expr::{
    BUILTIN_NAMES, Dim, LengthUnit, Quantity, Template, UNIT_NAMES, display_unit, escape_template,
    eval_str, is_valid_name, parse as parse_expr, rename_in_template, rename_reference,
    set_display_unit,
};

/// コア API の版。変える時は全モジュールの契約テストを通す。
pub const CORE_API_VERSION: &str = "0.39.0";

#[cfg(test)]
mod bodies_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod text_param_tests;
