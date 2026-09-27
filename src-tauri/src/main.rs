// Evita abrir uma janela de console extra no Windows em release. NÃO REMOVA!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    spiegel_lib::run();
}
