# 取り込み.ps1 が読み込む。取り込んだブランチを origin と手元から消す。
# origin のブランチの先頭が取り込んだコミット($Tips)のままの時だけ消す(--force-with-lease)。
# 取り込みの間に後からコミットが積まれていたら、消さずに残し、その名前を返す
# (素の push --delete は先頭を確かめないので、まだ取り込んでいないコミットまで消していた)。
function Remove-MergedBranch {
    param(
        [Parameter(Mandatory)][string]$Remote,
        # ブランチ名 → 取り込んだコミットの SHA
        [Parameter(Mandatory)][hashtable]$Tips
    )
    $kept = [Collections.Generic.List[string]]::new()
    foreach ($b in $Tips.Keys) {
        $sha = $Tips[$b]
        $out = & git.exe push --porcelain "--force-with-lease=refs/heads/${b}:$sha" $Remote ":refs/heads/$b" 2>&1
        if ($LASTEXITCODE -ne 0) {
            # 先頭が変わっている(stale info)か、ほかの理由で消せなかった。どちらも残して知らせる
            $why = ($out | Where-Object { "$_" -match '^(!|error|fatal)' } | Select-Object -First 1)
            $kept.Add("$b ($("$why".Trim()))")
            continue
        }
        & git.exe show-ref --verify --quiet "refs/heads/$b"
        if ($LASTEXITCODE -eq 0) { & git.exe branch -D $b *> $null }
    }
    , $kept
}
