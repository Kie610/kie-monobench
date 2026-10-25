//! 文書の中で重ならない名前を作る決まり。写し・視点・外観など、名前で並ぶ物がみな同じ付け方にする。

/// 使っていない名前。`name` が空いていればそのまま、使っていれば `名前 (2)`、`名前 (3)` … の
/// 空いている最初。`name` が既に `名前 (n)` の形なら番号を剥がしてから付ける(写しの写しを
/// 「名前 (2) (2)」にしない)。`taken` は名前が使われているか。
pub fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    let base = match name.rsplit_once(" (") {
        Some((b, k))
            if k.strip_suffix(')')
                .is_some_and(|n| n.parse::<u32>().is_ok()) =>
        {
            b
        }
        _ => name,
    };
    (2..)
        .map(|k| format!("{base} ({k})"))
        .find(|n| !taken(n))
        .expect("どこかで空く")
}

#[cfg(test)]
mod tests {
    use super::unique_name;

    #[test]
    fn numbers_follow_the_base_name() {
        let taken = ["ポケット1", "ポケット1 (2)"];
        let used = |n: &str| taken.contains(&n);
        assert_eq!(unique_name("輪郭1", used), "輪郭1");
        assert_eq!(unique_name("ポケット1", used), "ポケット1 (3)");
        assert_eq!(unique_name("ポケット1 (2)", used), "ポケット1 (3)");
        // 括弧の中が数でなければ名前の一部
        assert_eq!(unique_name("穴 (M3)", |n| n == "穴 (M3)"), "穴 (M3) (2)");
    }
}
