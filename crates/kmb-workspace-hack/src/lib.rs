//! 中身はない。全部の crate がこれに依存し、ここが依存先の機能を全体と同じにそろえる。
//! そうしないと `cargo test -p <crate>` のたびに、依存先が別の機能の組み合わせで作り直される。
//! 依存を足したら `cargo hakari generate` で作り直す(docs/verify.md)。
