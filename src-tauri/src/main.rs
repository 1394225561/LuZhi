// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = luzhi_lib::run() {
        eprintln!("录智启动失败：{error}");
        std::process::exit(1);
    }
}
