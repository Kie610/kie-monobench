<#
.SYNOPSIS
  クラウドのスレッドが作った claude/... ブランチを main へ取り込み、検査・プッシュ・後片付けまでを 1 回で行う。

.DESCRIPTION
  PC のコミット係が使う。流れは次のとおりで、どこかで失敗したらその場で止まり、以降(特にプッシュ)は行わない。
    1. origin から main と指定のブランチを取得し、main を origin/main に早送りする
    2. ブランチを順に取り込む(どれも merge --no-ff。ブランチ 1 本が 1 つの取り込みのコミットになり、戻す時に 1 回で戻せる)。
       衝突したら merge --abort して、そのブランチ名を出して止まる(それまでに取り込んだ分は手元の main に残る)
    2.5 cargo hakari generate --diff と manage-deps --dry-run で jc-workspace-hack がそろっているかを確かめる
    2.7 cargo-deny が無ければ入れる(verify --all のライセンスの点検が使う)
    3. cargo xtask verify --all(既定の SMLib の回)。出力は .local/取り込み/verify.log へ書き、画面には rc= と通過/失敗の行だけ出す
    4. 製品のコード(apps/ crates/ third_party/ plugins/ と、[patch] や依存を決める Cargo.toml・Cargo.lock、ビルドの設定の .cargo/config.toml・rust-toolchain.toml)が変わっていれば、アプリを release で作る(出力は .local/取り込み/build.log)。
       作るのは cargo xtask release(この PC のパスを exe に残さない引数を付けて作り、残っていないかを数える。前後の件数は要約に出す)。
       アプリ(kie-monobench.exe か前の名前の jisaku-cadcam.exe)が起動中で exe を上書きできないときは、代わりに cargo xtask release --check(cargo check)で確かめる。
       verify は debug でしか組み立てないので、release のときだけ出る失敗(写しの #![deny(warnings)] など)はここで拾い、落ちたらプッシュしない
    5. main を origin(Kie610/kie-monobench に限る)へプッシュする
    6. 取り込んだブランチを origin と手元から消す。origin の先頭が 1 で取得したコミットのままの時だけ消す(--force-with-lease)。
       取り込みの間にスレッドが後からコミットを積んでいたら、そのブランチは消さずに残し、名前を出す(後から積まれた分はまだ main に入っていない)
  最後に数行の要約を出す。終了コードは成功 0、失敗 1、main はプッシュしたが消さずに残したブランチがある 2。
  -WhatIf では取得(fetch)と衝突の下見(git merge-tree)だけを行い、取り込み・検査・ビルド・プッシュ・削除は行わない。
  手元の追跡中のファイルに変更があると止まる(未追跡の .local/ などは無視する。このスクリプトは git add をしない)。

.EXAMPLE
  pwsh scripts/取り込み.ps1 claude/foo-abc123 claude/bar-def456 -WhatIf
  pwsh scripts/取り込み.ps1 claude/foo-abc123 claude/bar-def456
#>
[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory, ValueFromRemainingArguments)]
    [string[]]$Branches
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$repo = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $repo
# 検査とビルドで PC のほかの作業を止めないよう、低い優先度で動かす(子の cargo・rustc にも引き継がれる)
(Get-Process -Id $PID).PriorityClass = 'Idle'
$env:CARGO_BUILD_JOBS = '12'
$logDir = Join-Path $repo '.local/取り込み'
New-Item -ItemType Directory -Force -Path $logDir -WhatIf:$false | Out-Null
$summary = [Collections.Generic.List[string]]::new()
. (Join-Path $PSScriptRoot '取り込み/ブランチの削除.ps1')

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
$Branches = $Branches | ForEach-Object { $_ -replace '^origin/', '' }
if ($Branches | Where-Object { $_ -notlike 'claude/*' }) { Stop-Run "claude/... 以外のブランチは扱わない: $($Branches -join ' ')" }
$url = (Invoke-Git remote get-url origin) | Select-Object -First 1
if ($url -notmatch '[:/]Kie610/kie-monobench(\.git)?$') { Stop-Run "origin が Kie610/kie-monobench ではない: $url" }
if ((Invoke-Git rev-parse --abbrev-ref HEAD) -ne 'main') { Stop-Run 'main にいない' }
if (Invoke-Git status --porcelain --untracked-files=no) { Stop-Run '追跡中のファイルに未コミットの変更がある' }

Invoke-Git fetch --prune origin main @Branches | Out-Null
# 取り込むコミットをここで控える。取り込み・削除はこの SHA で行い、後から積まれた分を取り違えない
$tips = [ordered]@{}
foreach ($b in $Branches) { $tips[$b] = (Invoke-Git rev-parse "origin/$b") | Select-Object -First 1 }
# -WhatIf では手元の main を動かさない。下見は、早送りした後の main(手元が origin/main より古ければ origin/main)と比べる
$base = 'HEAD'
if ($WhatIfPreference) {
    & git.exe merge-base --is-ancestor HEAD origin/main
    if ($LASTEXITCODE -eq 0) { $base = 'origin/main' }
    else {
        & git.exe merge-base --is-ancestor origin/main HEAD
        if ($LASTEXITCODE -ne 0) { Stop-Run '手元の main が origin/main から分かれている(早送りできない)' }
    }
} else {
    Invoke-Git merge --ff-only origin/main | Out-Null
}
$before = (Invoke-Git rev-parse HEAD) | Select-Object -First 1

# 1. 取り込み
if ($WhatIfPreference) {
    foreach ($b in $Branches) {
        $n = Invoke-Git rev-list --count "$base..origin/$b"
        & git.exe merge-tree --write-tree $base "origin/$b" *> $null
        # merge-tree は 0 = 衝突なし、1 = 衝突あり、それ以外 = 失敗
        $state = switch ($LASTEXITCODE) { 0 { '衝突なし' } 1 { '衝突あり' } default { Stop-Run "git merge-tree が失敗: $b" } }
        $summary.Add("取り込み(下見): $b 新しいコミット $n 件、main との$state")
    }
    $summary.Add('検査・ビルド・プッシュ・ブランチの削除: -WhatIf のため行わない')
    $summary | ForEach-Object { Write-Host $_ }
    exit 0
}
$i = 0
foreach ($b in $Branches) {
    $out = & git.exe merge --no-ff -m "$b を取り込み" $tips[$b] 2>&1
    if ($LASTEXITCODE -ne 0) {
        & git.exe merge --abort 2>&1 | Out-Null
        Stop-Run "$b の取り込みで衝突。merge を取り消した(先に取り込んだ $i 本は手元の main に残っている)`n$($out | Select-Object -Last 10 | Out-String)"
    }
    $i++
}
$summary.Add("取り込み: $($Branches.Count) 本 ($($Branches -join ', '))")

# 1.5 jc-workspace-hack が取り込んだ後の依存とそろっているか(docs/verify.md)。ずれていればプッシュせずに止める
foreach ($h in @(@('generate', '--diff'), @('manage-deps', '--dry-run'))) {
    $hout = & cargo hakari @h 2>&1
    if ($LASTEXITCODE -ne 0) {
        Stop-Run "cargo hakari $($h -join ' ') が rc=$LASTEXITCODE。cargo hakari generate と manage-deps --yes で直してコミットする(プッシュしていない)`n$($hout | Select-Object -Last 10 | Out-String)"
    }
}
$summary.Add('hakari: そろっている')

# 1.7 ライセンスの点検の道具(cargo-deny)。verify --all が使い、無ければ失敗にする。無ければ一度だけ入れる
& cargo deny --version *> $null
if ($LASTEXITCODE -ne 0) {
    Write-Host 'cargo-deny を入れます(初めの 1 回だけ。数分)'
    $dlog = Join-Path $logDir 'install-deny.log'
    & cargo install cargo-deny --locked --version 0.19.9 *> $dlog
    if ($LASTEXITCODE -ne 0) { Stop-Run "cargo-deny を入れられない(全文は $dlog)。プッシュしていない" }
}

# 2. 検査。出力は長いのでファイルへ書き、画面には結果の行だけを出す
$log = Join-Path $logDir 'verify.log'
& cargo xtask verify --all *> $log
$rc = $LASTEXITCODE
Write-Host "rc=$rc"
Select-String -LiteralPath $log -Pattern '^(通過|失敗) ' | ForEach-Object { Write-Host $_.Line }
if ($rc -ne 0) { Stop-Run "検査が失敗(全文は $log)。プッシュしていない" }
$summary.Add("検査: 通過 (rc=0、全文は $log)")

# 3. アプリの release ビルド。verify は debug だけなので、release だけの失敗をプッシュの前に拾う
# 起動中は exe を上書きできないので、組み立てずに cargo check --release で同じ確かめをする
# 比べる元は origin/main(前の回が止まって手元の main に取り込み済みの分も、まだプッシュしていないので数に入れる)
# 引用符なしの -- は PowerShell が関数の引数から取り除くので、文字列で渡す
$product = Invoke-Git diff --name-only origin/main HEAD '--' apps crates third_party plugins Cargo.toml Cargo.lock .cargo/config.toml rust-toolchain.toml
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
    if ($brc -ne 0) { Stop-Run "cargo xtask release $xargs が失敗(exe にパスが残った時も。全文は $blog)。プッシュしていない" }
    if ($running) {
        $summary.Add("ビルド: アプリが起動中のため cargo xtask release --check だけ通した(閉じてから cargo xtask release、全文は $blog)")
    } else {
        $summary.Add("ビルド: cargo xtask release 通過、$($paths.Line) (全文は $blog)")
    }
}

# 4. プッシュ
Invoke-Git push origin main | Out-Null
$summary.Add("プッシュ: main $($before.Substring(0,7))..$((Invoke-Git rev-parse --short HEAD))")

# 5. 取り込んだブランチの削除(origin と、手元にあれば手元も)。先頭が控えた SHA のままの物だけ消す
$kept = Remove-MergedBranch -Remote origin -Tips $tips
$summary.Add("ブランチの削除: $($Branches.Count - $kept.Count) 本")
foreach ($k in $kept) { $summary.Add("消さずに残した: $k。取り込みの後に積まれたコミットがあり、まだ main に入っていない。もう一度取り込む") }

Write-Host '== 取り込みのまとめ'
$summary | ForEach-Object { Write-Host $_ }
if ($kept.Count) { exit 2 }
exit 0
