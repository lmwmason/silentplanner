// SPDX-License-Identifier: Apache-2.0
//! Rank local map cells using the frozen linear research model.

use serde::{Deserialize, Serialize};

pub const MODEL: &str = "Exploratory linear regression";
pub const BASELINE_DBFS: f64 = -49.403511918841204;
pub const DATASET_DOI: &str = "10.5281/zenodo.23613504";
pub const LINEAR_INTERCEPT: f64 = -44.400288867444395;
pub const LINEAR_COEFFICIENTS: [f64; 3] =
    [-4.024117961216985, 0.10885137929908459, -0.8005196439319141];
pub const MAX_ZONES: usize = 32;
pub const MAX_CELLS: usize = 12_000;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    pub fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Bounds {
    pub min: Point,
    pub max: Point,
}

impl Bounds {
    pub fn spanning(a: Point, b: Point) -> Self {
        Self {
            min: Point::new(a.x.min(b.x), a.y.min(b.y)),
            max: Point::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    pub fn width(self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height(self) -> f64 {
        self.max.y - self.min.y
    }

    pub fn contains(self, point: Point) -> bool {
        (self.min.x..=self.max.x).contains(&point.x) && (self.min.y..=self.max.y).contains(&point.y)
    }

    pub fn expanded(self, margin: f64) -> Self {
        Self {
            min: Point::new(self.min.x - margin, self.min.y - margin),
            max: Point::new(self.max.x + margin, self.max.y + margin),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scene {
    #[default]
    Quiet,
    Roadside,
}

impl Scene {
    pub fn label(self) -> &'static str {
        match self {
            Self::Quiet => "Quiet outdoor",
            Self::Roadside => "Road-adjacent outdoor",
        }
    }

    pub fn max_distance(self) -> f64 {
        match self {
            Self::Quiet => 70.0,
            Self::Roadside => 10.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Zone {
    pub label: String,
    pub center: Point,
    pub radius_m: f64,
    pub weight: f64,
}

impl Zone {
    fn samples(&self) -> [Point; 9] {
        std::array::from_fn(|index| {
            if index == 0 {
                self.center
            } else {
                let angle = (index - 1) as f64 * std::f64::consts::TAU / 8.0;
                Point::new(
                    self.center.x + self.radius_m * angle.cos(),
                    self.center.y + self.radius_m * angle.sin(),
                )
            }
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Project {
    pub name: String,
    pub origin: Option<crate::satellite::Origin>,
    pub zones: Vec<Zone>,
    pub bounds: Option<Bounds>,
    pub scene: Scene,
    pub source_height_m: f64,
    pub cell_size_m: f64,
    pub selected_fraction: f64,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            name: "Untitled site".into(),
            origin: None,
            zones: Vec::new(),
            bounds: None,
            scene: Scene::Quiet,
            source_height_m: 15.0,
            cell_size_m: 2.0,
            selected_fraction: 0.15,
        }
    }
}

impl Project {
    pub fn demo() -> Self {
        Self {
            name: "Chungbuk Science High School".into(),
            origin: Some(crate::satellite::Origin::CHUNGBUK_SCIENCE_HIGH_SCHOOL),
            zones: vec![
                Zone {
                    label: "Example A".into(),
                    center: Point::new(-22.0, 14.0),
                    radius_m: 6.0,
                    weight: 1.0,
                },
                Zone {
                    label: "Example B".into(),
                    center: Point::new(15.0, 19.0),
                    radius_m: 7.0,
                    weight: 0.85,
                },
                Zone {
                    label: "Example C".into(),
                    center: Point::new(21.0, -16.0),
                    radius_m: 7.0,
                    weight: 0.6,
                },
            ],
            ..Self::default()
        }
    }

    pub fn auto_bounds(&self) -> Bounds {
        let Some(first) = self.zones.first() else {
            return Bounds::spanning(Point::new(-60.0, -42.0), Point::new(60.0, 42.0));
        };
        self.zones
            .iter()
            .fold(
                Bounds::spanning(first.center, first.center).expanded(first.radius_m),
                |bounds, zone| {
                    let footprint =
                        Bounds::spanning(zone.center, zone.center).expanded(zone.radius_m);
                    Bounds::spanning(
                        Point::new(
                            bounds.min.x.min(footprint.min.x),
                            bounds.min.y.min(footprint.min.y),
                        ),
                        Point::new(
                            bounds.max.x.max(footprint.max.x),
                            bounds.max.y.max(footprint.max.y),
                        ),
                    )
                },
            )
            .expanded((self.scene.max_distance() * 0.62).max(10.0))
    }

    pub fn view_bounds(&self) -> Bounds {
        let range = self.bounds.unwrap_or_else(|| self.auto_bounds());
        self.zones.iter().fold(range, |current, zone| {
            let footprint = Bounds::spanning(zone.center, zone.center).expanded(zone.radius_m);
            Bounds::spanning(
                Point::new(
                    current.min.x.min(footprint.min.x),
                    current.min.y.min(footprint.min.y),
                ),
                Point::new(
                    current.max.x.max(footprint.max.x),
                    current.max.y.max(footprint.max.y),
                ),
            )
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.origin.is_some_and(|origin| !origin.valid()) {
            return Err(
                "Use latitude between −80 and 80 and longitude between −179 and 179.".into(),
            );
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 100 {
            return Err("Use a project name between 1 and 100 characters.".into());
        }
        if !(5.0..=30.0).contains(&self.source_height_m)
            || !(0.5..=30.0).contains(&self.cell_size_m)
            || !(0.01..=0.5).contains(&self.selected_fraction)
        {
            return Err(
                "Height, cell size or selection percentage is outside the allowed range.".into(),
            );
        }
        if self.zones.len() > MAX_ZONES {
            return Err(format!(
                "A project supports up to {MAX_ZONES} occupied zones."
            ));
        }
        for zone in &self.zones {
            if !zone.center.finite()
                || zone.center.x.abs() > 10_000.0
                || zone.center.y.abs() > 10_000.0
                || !(0.0..=30.0).contains(&zone.radius_m)
                || !(0.1..=1.0).contains(&zone.weight)
                || zone.label.trim().is_empty()
                || zone.label.chars().count() > 60
            {
                return Err("A zone has an invalid position, radius, sensitivity or label.".into());
            }
        }
        let bounds = self.bounds.unwrap_or_else(|| self.auto_bounds());
        if !bounds.min.finite()
            || !bounds.max.finite()
            || !(self.cell_size_m..=1000.0).contains(&bounds.width())
            || !(self.cell_size_m..=1000.0).contains(&bounds.height())
            || [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y]
                .iter()
                .any(|p| p.abs() > 11_000.0)
        {
            return Err(
                "The analysis boundary must be finite and at least one cell wide and high.".into(),
            );
        }
        Ok(())
    }
}

pub fn predict_linear(horizontal_m: f64, height_m: f64, scene: Scene) -> Option<f64> {
    if !(5.0..=scene.max_distance()).contains(&horizontal_m) || !(5.0..=30.0).contains(&height_m) {
        return None;
    }
    let features = [
        horizontal_m.hypot(height_m).log10(),
        height_m,
        f64::from(u8::from(scene == Scene::Roadside)),
    ];
    Some(
        LINEAR_INTERCEPT
            + LINEAR_COEFFICIENTS
                .into_iter()
                .zip(features)
                .map(|(a, b)| a * b)
                .sum::<f64>(),
    )
}

#[derive(Clone, Debug, Serialize)]
pub struct Cell {
    pub center: Point,
    pub score: Option<f64>,
    pub selected: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Analysis {
    pub bounds: Bounds,
    pub columns: usize,
    pub rows: usize,
    pub cell_width_m: f64,
    pub cell_height_m: f64,
    pub cells: Vec<Cell>,
    pub scored_count: usize,
    pub selected_count: usize,
    pub score_range: Option<[f64; 2]>,
}

impl Analysis {
    pub fn cell_index(&self, point: Point) -> Option<usize> {
        if !self.bounds.contains(point) {
            return None;
        }
        let x = ((point.x - self.bounds.min.x) / self.cell_width_m).floor() as usize;
        let y = ((point.y - self.bounds.min.y) / self.cell_height_m).floor() as usize;
        Some(y.min(self.rows - 1) * self.columns + x.min(self.columns - 1))
    }

    pub fn cell_at(&self, point: Point) -> Option<&Cell> {
        self.cells.get(self.cell_index(point)?)
    }
}

pub fn analyze(project: &Project) -> Result<Analysis, String> {
    project.validate()?;
    let bounds = project.bounds.unwrap_or_else(|| project.auto_bounds());
    let columns = (bounds.width() / project.cell_size_m).ceil() as usize;
    let rows = (bounds.height() / project.cell_size_m).ceil() as usize;
    if columns * rows > MAX_CELLS {
        return Err(format!(
            "The grid exceeds {MAX_CELLS} cells. Increase cell size or draw a smaller range."
        ));
    }
    let width = bounds.width() / columns as f64;
    let height = bounds.height() / rows as f64;
    let occupied: Vec<_> = project
        .zones
        .iter()
        .map(|zone| (zone, zone.samples()))
        .collect();
    let score_at = |point: Point| -> Option<f64> {
        if occupied.is_empty() {
            return None;
        }
        let mut worst: f64 = 0.0;
        for (zone, samples) in &occupied {
            let center_distance = zone.center.distance(point);
            if center_distance - zone.radius_m < 5.0
                || center_distance + zone.radius_m > project.scene.max_distance()
            {
                return None;
            }
            let mut energy = 0.0;
            for receiver in samples {
                let dbfs = predict_linear(
                    point.distance(*receiver),
                    project.source_height_m,
                    project.scene,
                )?;
                energy += 10_f64.powf((dbfs - BASELINE_DBFS) / 10.0);
            }
            worst = worst.max(zone.weight * energy / samples.len() as f64);
        }
        Some(worst)
    };
    let mut cells = Vec::with_capacity(columns * rows);
    for y in 0..rows {
        for x in 0..columns {
            let center = Point::new(
                bounds.min.x + (x as f64 + 0.5) * width,
                bounds.min.y + (y as f64 + 0.5) * height,
            );
            cells.push(Cell {
                center,
                score: score_at(center),
                selected: false,
            });
        }
    }
    let mut order: Vec<_> = cells
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.score.map(|s| (i, s)))
        .collect();
    order.sort_by(|a, b| a.1.total_cmp(&b.1));
    let selected_count = (order.len() as f64 * project.selected_fraction).ceil() as usize;
    for &(index, _) in &order[..selected_count] {
        cells[index].selected = true;
    }
    let score_range = order.first().zip(order.last()).map(|(a, b)| [a.1, b.1]);
    Ok(Analysis {
        bounds,
        columns,
        rows,
        cell_width_m: width,
        cell_height_m: height,
        cells,
        scored_count: order.len(),
        selected_count,
        score_range,
    })
}

/// Shared map palette for the native canvas and the SVG export.
pub fn cell_color(cell: &Cell, analysis: &Analysis) -> [u8; 3] {
    if cell.selected {
        return [53, 218, 167];
    }
    let Some((value, [low, high])) = cell.score.zip(analysis.score_range) else {
        return [29, 41, 54];
    };
    let ratio = if high > low {
        (value - low) / (high - low)
    } else {
        0.0
    };
    let anchors = [
        [33.0, 84.0, 100.0],
        [172.0, 102.0, 59.0],
        [221.0, 112.0, 89.0],
    ];
    let step = if ratio < 0.5 { 0 } else { 1 };
    let fraction = ratio * 2.0 - step as f64;
    std::array::from_fn(|i| {
        (anchors[step][i] + fraction * (anchors[step + 1][i] - anchors[step][i])).round() as u8
    })
}
