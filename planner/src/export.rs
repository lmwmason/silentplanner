// SPDX-License-Identifier: Apache-2.0
//! Write a portable local map and reopen its editable project.

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::map::{
    Analysis, BASELINE_DBFS, Bounds, DATASET_DOI, LINEAR_COEFFICIENTS, LINEAR_INTERCEPT, MODEL,
    Point, Project, cell_color,
};

use crate::satellite::{self, ATTRIBUTION, ATTRIBUTION_URL, Origin};
use crate::sensitivity::{assess, field_plan};

const SCHEMA: u32 = 1;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectFile {
    schema_version: u32,
    project: Project,
}

pub fn open_project(path: &Path) -> Result<Project, String> {
    let input = fs::File::open(path).map_err(|error| format!("Could not open project: {error}"))?;
    if input.metadata().map_err(|error| error.to_string())?.len() > 256_000 {
        return Err("Project file is too large.".into());
    }
    let file: ProjectFile =
        serde_json::from_reader(input).map_err(|error| format!("Invalid project JSON: {error}"))?;
    if file.schema_version != SCHEMA {
        return Err(format!(
            "Unsupported project schema version: {}",
            file.schema_version
        ));
    }
    file.project.validate()?;
    Ok(file.project)
}

#[derive(Serialize)]
struct Report<'a> {
    schema_version: u32,
    purpose: &'static str,
    coordinate_system: &'static str,
    dataset_doi: &'static str,
    model: &'static str,
    coefficients: [f64; 3],
    intercept_dbfs: f64,
    selected_research_baseline_dbfs: f64,
    score_definition: &'static str,
    project: &'a Project,
    analysis: &'a Analysis,
}

pub fn export(project: &Project, output: &Path) -> Result<PathBuf, String> {
    write_bundle(project, output, None, None)
}

pub fn export_satellite(
    project: &Project,
    output: &Path,
) -> Result<(PathBuf, Option<String>), String> {
    project.validate()?;
    let (image, warning) = match project
        .origin
        .map(|origin| satellite::fetch(origin, export_view_bounds(project)))
    {
        Some(Ok(image)) => (Some(image), None),
        Some(Err(error)) => (None, Some(error)),
        None => (None, None),
    };
    let path = write_bundle(project, output, image.as_deref(), warning.as_deref())?;
    Ok((path, warning))
}

fn write_bundle(
    project: &Project,
    output: &Path,
    image: Option<&[u8]>,
    warning: Option<&str>,
) -> Result<PathBuf, String> {
    let analysis = crate::map::analyze(project)?;
    let stability = assess(project, &analysis);
    let field = field_plan(project, &analysis, &stability);
    let report = Report {
        schema_version: SCHEMA,
        purpose: "Exploratory relative acoustic exposure map",
        coordinate_system: "Local metres, x east, y north",
        dataset_doi: DATASET_DOI,
        model: MODEL,
        coefficients: LINEAR_COEFFICIENTS,
        intercept_dbfs: LINEAR_INTERCEPT,
        selected_research_baseline_dbfs: BASELINE_DBFS,
        score_definition: "Maximum across zones of sensitivity times mean relative digital energy across nine samples",
        project,
        analysis: &analysis,
    };
    let parent = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(output)
    };
    fs::create_dir_all(&parent)
        .map_err(|error| format!("Could not create export location: {error}"))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?;
    let directory = parent.join(format!("acoustic-map-{}", now.as_millis()));
    fs::create_dir(&directory)
        .map_err(|error| format!("Could not create export folder: {error}"))?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let write_pretty = |name: &str, bytes: Vec<u8>| fs::write(directory.join(name), bytes);
        write_pretty(
            "project.json",
            serde_json::to_vec_pretty(&ProjectFile {
                schema_version: SCHEMA,
                project: project.clone(),
            })?,
        )?;
        write_pretty("analysis.json", serde_json::to_vec_pretty(&report)?)?;
        fs::write(directory.join("map.svg"), svg(project, &analysis, image))?;
        fs::write(directory.join("cells.csv"), csv(&analysis))?;
        write_pretty("sensitivity.json", serde_json::to_vec_pretty(&stability)?)?;
        write_pretty("field_plan.json", serde_json::to_vec_pretty(&field)?)?;
        if let Some(origin) = project.origin {
            write_pretty(
                "impact.geojson",
                serde_json::to_vec_pretty(&geojson(project, &analysis, origin))?,
            )?;
        }
        if let Some(message) = warning {
            fs::write(
                directory.join("imagery-status.txt"),
                format!("{message}\nThe SVG uses the grid background.\n"),
            )?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&directory);
        return Err(format!("Could not export map: {error}"));
    }
    Ok(directory)
}

fn xml_escape(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .fold(String::new(), |mut output, c| {
            match c {
                '&' => output.push_str("&amp;"),
                '<' => output.push_str("&lt;"),
                '>' => output.push_str("&gt;"),
                '"' => output.push_str("&quot;"),
                _ => output.push(c),
            }
            output
        })
}

fn csv(analysis: &Analysis) -> String {
    let mut data = String::from("x_m,y_m,relative_impact,selected\n");
    for cell in &analysis.cells {
        let score = cell
            .score
            .map(|value| format!("{value:.8}"))
            .unwrap_or_default();
        writeln!(
            data,
            "{:.6},{:.6},{score},{}",
            cell.center.x, cell.center.y, cell.selected
        )
        .unwrap();
    }
    data
}

fn geojson(project: &Project, analysis: &Analysis, origin: Origin) -> Value {
    let ring = |bounds: Bounds| {
        [
            bounds.min,
            Point::new(bounds.max.x, bounds.min.y),
            bounds.max,
            Point::new(bounds.min.x, bounds.max.y),
            bounds.min,
        ]
        .map(|point| origin.lon_lat(point))
    };
    let mut features = vec![json!({
        "type": "Feature", "geometry": {"type": "Polygon", "coordinates": [ring(analysis.bounds)]},
        "properties": {"kind": "analysis_boundary", "project": project.name}
    })];
    for zone in &project.zones {
        features.push(json!({
            "type": "Feature", "geometry": {"type": "Point", "coordinates": origin.lon_lat(zone.center)},
            "properties": {"kind": "occupied_zone", "label": zone.label, "radius_m": zone.radius_m,
                "sensitivity": zone.weight}
        }));
    }
    let half_width = analysis.cell_width_m / 2.0;
    let half_height = analysis.cell_height_m / 2.0;
    for cell in analysis.cells.iter().filter(|cell| cell.selected) {
        let bounds = Bounds::spanning(
            Point::new(cell.center.x - half_width, cell.center.y - half_height),
            Point::new(cell.center.x + half_width, cell.center.y + half_height),
        );
        features.push(json!({
            "type": "Feature", "geometry": {"type": "Polygon", "coordinates": [ring(bounds)]},
            "properties": {"kind": "selected_low_impact_cell", "relative_impact": cell.score}
        }));
    }
    json!({"type": "FeatureCollection", "features": features})
}

fn export_view_bounds(project: &Project) -> Bounds {
    let context = if project.origin.is_some() { 55.0 } else { 0.0 };
    project.view_bounds().expanded(context)
}

fn svg(project: &Project, analysis: &Analysis, image: Option<&[u8]>) -> String {
    let world = export_view_bounds(project);
    let scale = 800.0 / world.width().max(world.height());
    let width = world.width() * scale;
    let height = world.height() * scale;
    let origin = Point::new(46.0 + (800.0 - width) / 2.0, 100.0);
    let map = |p: Point| {
        Point::new(
            origin.x + (p.x - world.min.x) * scale,
            origin.y + (world.max.y - p.y) * scale,
        )
    };
    let mut out = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 892 {canvas:.1}" role="img">
<rect width="100%" height="100%" fill="#0d1520"/>
<g font-family="sans-serif" fill="#eaf4f8">
<text x="46" y="45" font-size="29" font-weight="700">{title}</text>
<text x="46" y="73" font-size="14" fill="#b4cad8">Relative digital-energy map • {scene} • source height {alt:.1} m</text>
</g>"##,
        canvas = height + 236.0,
        title = xml_escape(&project.name),
        scene = project.scene.label(),
        alt = project.source_height_m,
    );
    writeln!(
        out,
        r##"<rect x="{:.3}" y="{:.3}" width="{width:.3}" height="{height:.3}" fill="#141f2d"/>"##,
        origin.x, origin.y
    )
    .unwrap();
    if let Some(jpeg) = image {
        let encoded = STANDARD.encode(jpeg);
        writeln!(out, r##"<image x="{:.3}" y="{:.3}" width="{width:.3}" height="{height:.3}" preserveAspectRatio="none" href="data:image/jpeg;base64,{encoded}"/>"##,
            origin.x, origin.y).unwrap();
    }
    let cw = analysis.cell_width_m * scale;
    let ch = analysis.cell_height_m * scale;
    for cell in &analysis.cells {
        let point = map(cell.center);
        let [r, g, b] = cell_color(cell, analysis);
        let alpha = if image.is_none() {
            1.0
        } else if cell.selected {
            0.73
        } else if cell.score.is_some() {
            0.56
        } else {
            0.23
        };
        writeln!(out, r##"<rect x="{:.3}" y="{:.3}" width="{cw:.3}" height="{ch:.3}" fill="#{r:02x}{g:02x}{b:02x}" fill-opacity="{alpha:.2}"/>"##,
            point.x - cw / 2.0, point.y - ch / 2.0).unwrap();
    }
    let a = map(Point::new(analysis.bounds.min.x, analysis.bounds.max.y));
    writeln!(out, r##"<rect x="{:.3}" y="{:.3}" width="{:.3}" height="{:.3}" fill="none" stroke="#668c9f" stroke-width="1"/>"##,
        a.x, a.y, analysis.bounds.width() * scale, analysis.bounds.height() * scale).unwrap();
    for zone in &project.zones {
        let point = map(zone.center);
        writeln!(out, r##"<circle cx="{:.2}" cy="{:.2}" r="{:.2}" fill="#211e41" fill-opacity=".5" stroke="#e1c6ff" stroke-width="2"/><text x="{:.2}" y="{:.2}" fill="#fff5ff" stroke="#142031" stroke-width="3" paint-order="stroke" font-family="sans-serif" font-size="13">{}</text>"##,
            point.x, point.y, (zone.radius_m * scale).max(3.0), point.x + 5.0, point.y - 6.0, xml_escape(&zone.label)).unwrap();
    }
    let footer = height + 133.0;
    let legend = [
        ("#35daa7", "Selected low-score cells"),
        ("#dd7059", "Higher impact"),
        ("#1d2936", "Outside model support"),
        ("#e1c6ff", "Occupied zone"),
    ];
    for (index, (color, label)) in legend.iter().enumerate() {
        let x = 46.0 + index as f64 * 195.0;
        writeln!(out, r##"<rect x="{x:.1}" y="{footer:.1}" width="12" height="12" rx="2" fill="{color}"/><text x="{:.1}" y="{:.1}" fill="#eaf4f8" font-family="sans-serif" font-size="12">{label}</text>"##,
            x + 19.0, footer + 11.0).unwrap();
    }
    writeln!(out, r##"<path d="M46 {0:.1} h{1:.1}" stroke="#b4cad8" stroke-width="2"/><text x="{2:.1}" y="{3:.1}" fill="#b4cad8" font-family="sans-serif" font-size="12">{4:.0} m • north up</text><text x="46" y="{5:.1}" fill="#b4cad8" font-family="sans-serif" font-size="12">Exploratory outdoor proxy • No calibrated SPL or dBA • DOI {6}</text>"##,
        footer + 35.0, scale * if world.width() >= 40.0 { 10.0 } else { 1.0 },
        58.0 + scale * if world.width() >= 40.0 { 10.0 } else { 1.0 }, footer + 39.0,
        if world.width() >= 40.0 { 10.0 } else { 1.0 }, footer + 67.0, DATASET_DOI).unwrap();
    if image.is_some() {
        writeln!(out, r##"<a href="{ATTRIBUTION_URL}"><text x="46" y="{:.1}" fill="#b4cad8" font-family="sans-serif" font-size="11">{}</text></a>"##,
            footer + 83.0, xml_escape(ATTRIBUTION)).unwrap();
    }
    out.push_str("</svg>\n");
    out
}
