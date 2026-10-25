//! 差し込むもの一式。機能の種類の表と形状カーネルを、組み立て(jc-bundle)から
//! 画面・CLI・MCP へ 1 つの値で渡す。受け取る側は具体的なカーネルを知らない。

use crate::{Extension, Extensions, Kernel, Registry};
use std::sync::Arc;

/// 使う機能の種類、形状カーネル、モジュールの操作。複製は安い(中身は共有)。
#[derive(Clone)]
pub struct Modules {
    pub registry: Arc<Registry>,
    pub kernel: Arc<dyn Kernel>,
    pub extensions: Extensions,
}

impl Modules {
    pub fn new(registry: Registry, kernel: impl Kernel + 'static) -> Self {
        Self {
            registry: Arc::new(registry),
            kernel: Arc::new(kernel),
            extensions: Extensions::default(),
        }
    }

    /// モジュールの操作を足す。
    pub fn with_extension(mut self, e: impl Extension + 'static) -> Self {
        self.extensions.0.push(Arc::new(e));
        self
    }
}

/// 1 つのモジュールの名乗り: 機能の種類の登録と、画面・CLI・MCP に出す操作。
/// 各モジュールの crate が `module()` で 1 つ出し、組み立て(jc-bundle)が 1 回ずつ並べる。
pub struct Module {
    /// crate の名前(例: `jc-cad-hole`)
    pub name: &'static str,
    /// 機能の種類を登録する
    pub register: Arc<dyn Fn(&mut Registry) + Send + Sync>,
    /// モジュールの操作
    pub extensions: Vec<Arc<dyn Extension>>,
}

impl Module {
    pub fn new(
        name: &'static str,
        register: impl Fn(&mut Registry) + Send + Sync + 'static,
    ) -> Self {
        Self {
            name,
            register: Arc::new(register),
            extensions: Vec::new(),
        }
    }

    /// 種類を持たず、操作だけのモジュール。
    pub fn commands(name: &'static str) -> Self {
        Self::new(name, |_| {})
    }

    /// モジュールの操作を足す。
    pub fn with_extension(mut self, e: impl Extension + 'static) -> Self {
        self.extensions.push(Arc::new(e));
        self
    }
}

impl std::fmt::Debug for Module {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Module")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// モジュールの種類を順に登録した表。
pub fn registry_of(modules: &[Module]) -> Registry {
    let mut r = Registry::new();
    for m in modules {
        (m.register)(&mut r);
    }
    r
}

impl Modules {
    /// モジュールの一覧とカーネルから一式を作る。種類と操作はモジュールの順に並ぶ。
    pub fn assemble(modules: Vec<Module>, kernel: Arc<dyn Kernel>) -> Self {
        let registry = registry_of(&modules);
        Self {
            registry: Arc::new(registry),
            kernel,
            extensions: Extensions(modules.into_iter().flat_map(|m| m.extensions).collect()),
        }
    }
}

impl std::fmt::Debug for Modules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Modules").finish_non_exhaustive()
    }
}
