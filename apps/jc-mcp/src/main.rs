//! `kmb-mcp [--root <作業フォルダ>] [--allow <フォルダ>]...`。MCP クライアントの設定に、
//! このコマンドを登録して使う。AI が読み書きできるのは、作業フォルダと `--allow` のフォルダだけ。

use rmcp::ServiceExt;
use std::path::PathBuf;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut allow: Vec<PathBuf> = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--allow" => match args.next() {
                Some(d) => allow.push(PathBuf::from(d)),
                None => {
                    eprintln!("--allow の後に許すフォルダを書いてください");
                    return ExitCode::FAILURE;
                }
            },
            "--root" => match args.next() {
                Some(r) => root = PathBuf::from(r),
                None => {
                    eprintln!("--root の後に作業フォルダを書いてください");
                    return ExitCode::FAILURE;
                }
            },
            "--help" | "-h" => {
                println!(
                    "kmb-mcp [--root <作業フォルダ>] [--allow <フォルダ>]...  標準入出力で MCP サーバとして動く"
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("知らない引数です: {other}");
                return ExitCode::FAILURE;
            }
        }
    }
    if !root.is_dir() {
        eprintln!("作業フォルダ {} がありません", root.display());
        return ExitCode::FAILURE;
    }
    // ログとクラッシュ報告を手元に残す(送信しない)。残せなくても動く
    let data_dir = jc_session::default_data_dir();
    jc_session::logging::init(data_dir.as_deref(), "kmb-mcp");
    log::info!("作業フォルダ {}、許すフォルダ {allow:?}", root.display());
    let _crash = data_dir.as_deref().and_then(|d| {
        jc_session::crash::install(d, "kmb-mcp")
            .map_err(|e| eprintln!("クラッシュ報告を残せません: {e}"))
            .ok()
    });
    let mut session = jc_mcp::Session::new(root);
    if let Some(d) = data_dir {
        session = session.with_data_dir(d);
    }
    for d in &allow {
        if let Err(e) = session.allow(d) {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    }
    jc_session::apply_user_settings(session.modules());
    let server = jc_mcp::Server::new(session);
    // 標準出力は MCP の通信に使うので、ログは標準エラーへ
    // 改行の無い大きすぎる送り物で記憶を食い尽くさない(10-24 m3)
    let (stdin, stdout) = rmcp::transport::stdio();
    let stdin = jc_mcp::line_cap::LineCap::new(stdin, jc_mcp::line_cap::MAX_LINE);
    match server.serve((stdin, stdout)).await {
        Ok(running) => {
            if let Err(e) = running.waiting().await {
                eprintln!("終了: {e}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("起動できません: {e}");
            ExitCode::FAILURE
        }
    }
}
