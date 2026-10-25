//! 打ち間違えた名前に近い名前を選ぶ(操作の ID など。画面・CLI・MCP の誤りで同じ物を使う)。

/// `name` に近い順に、`candidates` から 3 つまで(近い物が無ければ空)。字の入れ替え・足し・
/// 抜けの数(編集距離)が名前の長さの 1/3(少なくとも 2)まで、または一方がもう一方を含む物。
pub fn near_names<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let want: Vec<char> = name.to_lowercase().chars().collect();
    let limit = (want.len() / 3).max(2);
    let mut found: Vec<(usize, &str)> = candidates
        .into_iter()
        .filter(|c| *c != name)
        .filter_map(|c| {
            let low = c.to_lowercase();
            let d = distance(&want, &low.chars().collect::<Vec<_>>());
            let inside = !want.is_empty()
                && (low.contains(&name.to_lowercase()) || name.to_lowercase().contains(&low));
            (d <= limit || inside).then_some((d, c))
        })
        .collect();
    found.sort();
    found.dedup();
    found
        .into_iter()
        .take(3)
        .map(|(_, c)| c.to_string())
        .collect()
}

/// 編集距離(字の入れ替え・足し・抜けの数)。
fn distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// 誤りの文に足す「近い名前」の案内(近い物が無ければ空の文字)。
pub fn hint(near: &[String]) -> String {
    if near.is_empty() {
        String::new()
    } else {
        format!("。近い名前: {}", near.join("、"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mistyped_name_finds_the_close_ones_first() {
        let ids = [
            "io.export_stl",
            "io.export_step",
            "cad.measure",
            "asm.joint",
        ];
        assert_eq!(
            near_names("io.exprot_stl", ids),
            ["io.export_stl", "io.export_step"]
        );
        assert_eq!(near_names("measure", ids), ["cad.measure"]);
        assert!(near_names("全く違う", ids).is_empty());
    }
}
