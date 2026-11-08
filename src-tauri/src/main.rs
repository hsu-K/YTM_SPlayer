// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// The binary is deliberately named "YTM_SPlayer" (not snake_case).
#![allow(non_snake_case)]

fn main() {
    ytm_splayer_lib::run()
}
