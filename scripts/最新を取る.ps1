<#
.SYNOPSIS
  GitHub で取り込んだ main を PC に取ってきて、アプリを release で作り直す。

.DESCRIPTION
  PC のコミット係が使う。取り込みは GitHub の上で済んでいる(docs/verify.md の 16 節)ので、ここでは取り込みも
  プッシュもしない。流れは次のとおりで、どこかで失敗したらその場で止まる。
    1. origin から main を取得し、手元の main を origin/main に早送りする(手元だけのコミットがあれば止まる)
    2. 前の手元の main から後に GitHub で取り込んだ PR の一覧(番号・題・URL)を出す。これをそのまま報告に使う
    3. 製品のコード(apps/ crates/ third_party/ plugins/ と、[patch] や依存を決める Cargo.toml・Cargo.lock、ビルドの設定の
       .cargo/config.toml・rust-toolchain.toml)が変わっていれば、cargo xtask release でアプリを作る(出力は .local/最新/build.log)。
       アプリ(kie-monobench.exe か前の名前の jisaku-cadcam.exe)が起動中で exe を上書きできないときは、
       代わりに cargo xtask release --check(cargo check)で確かめる。
       CI の検査は debug だけなので、release のときだけ出る失敗(写しの #![deny(warnings)] など)はここで拾う。
       落ちたら、取り込んだ PR の一覧と一緒に報告し、直すのは新しいスレッドで行う(main はもう GitHub にある)。
  最後に数行の要約を出す。終了コードは成功 0、失敗 1。
  -WhatIf では取得と一覧だけを行い、手元の main を動かさず、ビルドもしない。
  手元の追跡中のファイルに変更があると止まる(未追跡の .local/ などは無視する)。

.EXAMPLE
  pwsh scripts/最新を取る.ps1 -WhatIf
  pwsh scripts/最新を取る.ps1
#>
[CmdletBinding(SupportsShouldProcess)]
param()

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$repo = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $repo
# ビルドで PC のほかの作業を止めないよう、低い優先度で動かす(子の cargo・rustc にも引き継がれる)
(Get-Process -Id $PID).PriorityClass = 'Idle'
$env:CARGO_BUILD_JOBS = '12'
$logDir = Join-Path $repo '.local/最新'
New-Item -ItemType Directory -Force -Path $logDir -WhatIf:$false | Out-Null
$summary = [Collections.Generic.List[string]]::new()

function Stop-Run([string]$why) {
    Write-Host "止めました: $why" -ForegroundColor Red
    $summary | ForEach-Object { Write-Host "  $_" }
    exit 1
}

# git を動かし、失敗したら止まる。出力は呼び出し元へ返す
function Invoke-Git {
    $out = & git.exe @args 2>&1
    $rc = $LASTEXITCODE
    if ($rc -ne 0) { Stop-Run "git $($args -join ' ') (rc=$rc)`n$($out | Select-Object -Last 10 | Out-String)" }
    $out
}

# 前提: main にいて、追跡中のファイルに変更がなく、origin が許可されたリポジトリであること
$url = (Invoke-Git remote get-url origin) | Select-Object -First 1
if ($url -notmatch '[:/]Kie610/kie-monobench(\.git)?$') { Stop-Run "origin が Kie610/kie-monobench ではない: $url" }
if ((Invoke-Git rev-parse --abbrev-ref HEAD) -ne 'main') { Stop-Run 'main にいない' }
if (Invoke-Git status --porcelain --untracked-files=no) { Stop-Run '追跡中のファイルに未コミットの変更がある' }

# 1. 取得と早送り。GitHub で消えた claude/... の追跡の枝も片付ける
Invoke-Git fetch --prune origin main | Out-Null
$before = (Invoke-Git rev-parse HEAD) | Select-Object -First 1
& git.exe merge-base --is-ancestor HEAD origin/main
if ($LASTEXITCODE -ne 0) { Stop-Run '手元の main に origin/main に無いコミットがある(早送りできない)。GitHub の PR にしてから取り込む' }
$after = (Invoke-Git rev-parse origin/main) | Select-Object -First 1
if ($before -eq $after) {
    Write-Host '新しい取り込みはありません'
    exit 0
}

# 2. 取り込んだ PR の一覧。GitHub の取り込みのコミットは、題が「Merge pull request #<番号> from <ブランチ>」、本文の 1 行目が PR の題
$base = ($url -replace '\.git$', '' -replace '^git@github\.com:', 'https://github.com/')
$merges = Invoke-Git log --first-parent --merges '--format=%s%x1f%b%x1e' "$before..$after"
$prs = [Collections.Generic.List[string]]::new()
$rs, $us = [string][char]0x1e, [string][char]0x1f
foreach ($m in (($merges | Out-String) -split $rs)) {
    $subject, $body = $m.Trim() -split $us, 2
    if (-not $subject) { continue }
    if ($subject -match '^Merge pull request #(\d+) from \S+') {
        $title = ($body -split "`n" | Where-Object { $_.Trim() } | Select-Object -First 1)
        $prs.Add("#$($Matches[1]) $title $base/pull/$($Matches[1])")
    } else {
        $prs.Add($subject)
    }
}
$n = Invoke-Git rev-list --count "$before..$after"
$summary.Add("取り込んだ物: $($prs.Count) 件(コミット $n 件、$($before.Substring(0,7))..$($after.Substring(0,7)))")
foreach ($p in $prs) { $summary.Add("  $p") }

$product = Invoke-Git diff --name-only $before $after '--' apps crates third_party plugins Cargo.toml Cargo.lock .cargo/config.toml rust-toolchain.toml
if ($WhatIfPreference) {
    $summary.Add("ビルド: -WhatIf のため行わない(製品のコードの変更 $(@($product).Count) ファイル)")
    $summary | ForEach-Object { Write-Host $_ }
    exit 0
}
Invoke-Git merge --ff-only origin/main | Out-Null

# 3. アプリの release ビルド。起動中は exe を上書きできないので、組み立てずに cargo check --release で同じ確かめをする
if (-not $product) {
    $summary.Add('ビルド: 製品のコードに変更なし')
} else {
    $blog = Join-Path $logDir 'build.log'
    $running = Get-Process -Name 'kie-monobench', 'jisaku-cadcam' -ErrorAction SilentlyContinue
    # exe にこの PC のパス(ユーザー名を含むホームなど)を残さない引数は xtask が付ける(xtask/src/release.rs)
    $xargs = if ($running) { @('--check') } else { @() }
    & cargo xtask release @xargs *> $blog
    $brc = $LASTEXITCODE
    Write-Host "cargo xtask release $xargs rc=$brc"
    $paths = Select-String -LiteralPath $blog -Pattern '^パス: ' | Select-Object -Last 1
    if ($paths) { Write-Host $paths.Line }
    if ($brc -ne 0) { Stop-Run "cargo xtask release $xargs が失敗(exe にパスが残った時も。全文は $blog)。上の PR のどれかを新しいスレッドで直す" }
    if ($running) {
        $summary.Add("ビルド: アプリが起動中のため cargo xtask release --check だけ通した(閉じてから cargo xtask release、全文は $blog)")
    } else {
        $summary.Add("ビルド: cargo xtask release 通過、$($paths.Line) (全文は $blog)")
    }
}

Write-Host '== 最新を取ったまとめ'
$summary | ForEach-Object { Write-Host $_ }
exit 0
