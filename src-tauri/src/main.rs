// Release builds are desktop applications, not console programs on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    utterform_lib::run();
}
