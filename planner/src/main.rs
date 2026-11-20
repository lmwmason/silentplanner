// SPDX-License-Identifier: Apache-2.0
//! Launch the native planner or export a reference map without a display.

mod appearance;
mod place;
mod tile_map;
mod ui;

use std::{ffi::OsStr, path::Path};

use eframe::egui;
use silent_planner::{export, map::Project};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    match arguments.as_slice() {
        [command, output] if command == OsStr::new("--demo-offline") => {
            println!(
                "{}",
                export::export(&Project::demo(), Path::new(output))?.display()
            );
        }
        [] => {
            let options = eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size([1390.0, 870.0])
                    .with_min_inner_size([960.0, 650.0]),
                ..Default::default()
            };
            eframe::run_native(
                "Silent Planner",
                options,
                Box::new(|context| Ok(Box::new(ui::Planner::new(&context.egui_ctx)))),
            )?;
        }
        [command, output] if command == OsStr::new("--demo-export") => {
            let (directory, warning) =
                export::export_satellite(&Project::demo(), Path::new(output))?;
            if let Some(warning) = warning {
                eprintln!("{warning}");
            }
            println!("{}", directory.display());
        }
        [command, input, output] if command == OsStr::new("--export") => {
            let project = export::open_project(Path::new(input))?;
            let (directory, warning) = export::export_satellite(&project, Path::new(output))?;
            if let Some(warning) = warning {
                eprintln!("{warning}");
            }
            println!("{}", directory.display());
        }
        _ => {
            return Err(
                "Usage: silent-planner [--demo-offline DIRECTORY | --demo-export DIRECTORY | --export PROJECT DIRECTORY]"
                    .into(),
            );
        }
    }
    Ok(())
}
