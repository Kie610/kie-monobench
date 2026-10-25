# ブランチの削除.ps1 の試験。git だけで動く(cargo は要らない)。
#   pwsh -File scripts/取り込み/ブランチの削除.test.ps1
# 仮の origin(bare)と手元を作り、次の 2 つを確かめる。
#   1. 取り込んだ後に何も積まれていなければ、origin と手元のブランチが消える
#   2. 取り込みの間に別のコミットが積まれていれば、消さずに残し、その名前を返す
# -Impl で別の実装を渡せる(修正前の実装で 2 が落ちることを確かめるため)
param([string]$Impl = (Join-Path $PSScriptRoot 'ブランチの削除.ps1'))
$ErrorActionPreference = 'Stop'
. $Impl

$root = Join-Path ([IO.Path]::GetTempPath()) "jc-branch-delete-$PID"
New-Item -ItemType Directory -Force -Path $root | Out-Null
$failed = 0
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Host "通過 $what" } else { Write-Host "失敗 $what"; $script:failed++ }
}
function G { & git.exe @args 2>&1; if ($LASTEXITCODE -ne 0) { throw "git $args" } }
function Commit([string]$msg) {
    Set-Content -LiteralPath (Join-Path (Get-Location) "$msg.txt") -Value $msg
    G add -A | Out-Null
    G -c user.name=t -c user.email=t@t commit -q -m $msg | Out-Null
}
try {
    G init -q --bare -b main (Join-Path $root 'origin.git') | Out-Null
    G clone -q (Join-Path $root 'origin.git') (Join-Path $root 'pc') | Out-Null
    G clone -q (Join-Path $root 'origin.git') (Join-Path $root 'cloud') | Out-Null
    Push-Location (Join-Path $root 'cloud')
    G checkout -q -b main | Out-Null
    Commit 'base'; G push -q origin main | Out-Null
    foreach ($b in 'claude/quiet', 'claude/busy') {
        G checkout -q -b $b main | Out-Null
        Commit "$($b -replace '/', '-')-1"; G push -q origin $b | Out-Null
    }
    Pop-Location

    # PC: 取り込み.ps1 と同じく取得して先頭を控え、取り込む
    Push-Location (Join-Path $root 'pc')
    G fetch -q origin main claude/quiet claude/busy | Out-Null
    G checkout -q -b claude/quiet origin/claude/quiet | Out-Null
    G checkout -q -B main origin/main | Out-Null
    $tips = @{}
    foreach ($b in 'claude/quiet', 'claude/busy') { $tips[$b] = (G rev-parse "origin/$b" | Select-Object -First 1) }
    Pop-Location

    # 検査の間に、クラウドのスレッドが claude/busy へもう 1 つ積む
    Push-Location (Join-Path $root 'cloud')
    G checkout -q claude/busy | Out-Null
    Commit 'busy-2'; G push -q origin claude/busy | Out-Null
    $late = G rev-parse HEAD | Select-Object -First 1
    Pop-Location

    Push-Location (Join-Path $root 'pc')
    $kept = Remove-MergedBranch -Remote origin -Tips $tips
    $remote = G ls-remote origin 'refs/heads/claude/*'
    Pop-Location

    Check (-not ($remote -match 'refs/heads/claude/quiet$')) '1. 何も積まれていないブランチは origin から消える'
    Push-Location (Join-Path $root 'pc')
    & git.exe show-ref --verify --quiet refs/heads/claude/quiet
    Check ($LASTEXITCODE -ne 0) '1. 手元のブランチも消える'
    Pop-Location
    Check ([bool]($remote -match "^$late\s+refs/heads/claude/busy$")) '2. 後から積まれたコミットは origin に残る'
    Check ([bool]($kept | Where-Object { $_ -like 'claude/busy*' })) '2. 残したブランチの名前を返す'
    Check (-not ($kept | Where-Object { $_ -like 'claude/quiet*' })) '2. 消したブランチは返さない'
} finally {
    Set-Location $PSScriptRoot
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
if ($failed) { Write-Host "失敗 $failed 件"; exit 1 }
Write-Host '全て通過'
exit 0
