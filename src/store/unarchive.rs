use super::{err, plain_path, Result, Store};
use std::env;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(super) fn run(store: &Store, id: &str) -> Result<Output> {
    let cli = on_path("codex.exe")
        .or_else(desktop_codex)
        .or_else(|| on_path("codex.cmd"))
        .ok_or("找不到 Codex 命令行程序，请在 ChatGPT 中取消归档")?;
    let home = plain_path(&fs::canonicalize(&store.home).map_err(err)?);
    let sqlite = store.state.parent().ok_or("缺少会话数据库目录")?;
    let sqlite = plain_path(&fs::canonicalize(sqlite).map_err(err)?);
    hidden(&cli)
        .args(["unarchive", id])
        .current_dir(&home)
        .env("CODEX_HOME", &home)
        .env("CODEX_SQLITE_HOME", sqlite)
        .output()
        .map_err(|e| format!("无法启动 Codex：{e}"))
}

fn on_path(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

fn desktop_codex() -> Option<PathBuf> {
    let windows = PathBuf::from(env::var_os("SystemRoot")?);
    let powershell = windows.join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let output = hidden(&powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); \
             Get-AppxPackage -Name 'OpenAI.Codex' | Select-Object -First 1 -ExpandProperty InstallLocation",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let location = std::str::from_utf8(&output.stdout).ok()?.trim();
    let cli = Path::new(location).join(r"app\resources\codex.exe");
    (cli.is_absolute() && cli.is_file()).then_some(cli)
}

fn hidden(program: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null());
    command
}
