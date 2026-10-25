//! 見た目と材料の語彙。形を変えずに見た目と質量だけに効く物を、コアの外に置く(描き手・解析・
//! CAD・加工が共通で引くので、どの層からも使えるコアの層の小部品にする)。
//!
//! - 物理材料と外観の表、ボディに付けた材料の区画(`cad.materials`): [`material`]
//! - 文書の外観(「このデザイン内」の外観、ボディ・部品・面の外観)の区画(`render.appearances`):
//!   [`appearance`]
//! - 光源とデカール(タイムラインの機能が出し、描き手が型で読む): [`decor`]

pub mod appearance;
pub mod decor;
pub mod material;
