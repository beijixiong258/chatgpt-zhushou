#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod desktop;
mod items;
mod store;
mod web;

fn main() {
    desktop::start();
}
