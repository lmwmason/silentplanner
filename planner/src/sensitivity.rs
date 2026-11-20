// SPDX-License-Identifier: Apache-2.0
//! Compare map rankings under plausible changes in analytical assumptions.

use serde::Serialize;

use crate::map::{Analysis, Point, Project, analyze};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    StableLow,
    StableOther,
    Uncertain,
    Unsupported,
}

#[derive(Debug, Serialize)]
pub struct CellStability {
    pub center: Point,
    pub supported: usize,
    pub selected: usize,
    pub class: Class,
}

#[derive(Debug, Serialize)]
pub struct Sensitivity {
    pub completed: Vec<&'static str>,
    pub skipped: Vec<String>,
    pub cells: Vec<CellStability>,
}

#[derive(Debug, Serialize)]
pub struct FieldPoint {
    pub point: Point,
    pub modeled_score: f64,
}

#[derive(Debug, Serialize)]
pub struct FieldPair {
    pub id: String,
    pub lower: FieldPoint,
    pub higher: FieldPoint,
}

#[derive(Debug, Serialize)]
pub struct FieldPlan<'a> {
    pub project_name: &'a str,
    pub coordinate_system: &'static str,
    pub origin: Option<crate::satellite::Origin>,
    pub source_height_m: f64,
    pub receiver_zones: &'a [crate::map::Zone],
    pub receiver_sampling: &'static str,
    pub pairs: Vec<FieldPair>,
}

pub fn assess(project: &Project, reference: &Analysis) -> Sensitivity {
    let mut completed = vec!["reference"];
    let mut skipped = Vec::new();
    let mut counts: Vec<_> = reference
        .cells
        .iter()
        .map(|cell| {
            (
                usize::from(cell.score.is_some()),
                usize::from(cell.selected),
            )
        })
        .collect();
    let scenarios = [
        ("radius_minus_25_pct", 0.75, 0, 1.0),
        ("radius_plus_25_pct", 1.25, 0, 1.0),
        ("alternating_sensitivity_a", 1.0, 1, 1.0),
        ("alternating_sensitivity_b", 1.0, -1, 1.0),
        ("grid_finer_25_pct", 1.0, 0, 0.75),
        ("grid_coarser_25_pct", 1.0, 0, 1.25),
    ];
    let mut original = project.clone();
    original.bounds = Some(reference.bounds);
    for (name, radius, weight, spacing) in scenarios {
        let mut variant = original.clone();
        variant.cell_size_m = (variant.cell_size_m * spacing).clamp(0.5, 30.0);
        for (index, zone) in variant.zones.iter_mut().enumerate() {
            zone.radius_m = (zone.radius_m * radius).clamp(0.0, 30.0);
            let parity = if index % 2 == 0 { 1 } else { -1 };
            zone.weight = (zone.weight * (1.0 + f64::from(parity * weight) * 0.25)).clamp(0.1, 1.0);
        }
        if variant == original {
            skipped.push(format!("{name}: unchanged after applying limits"));
            continue;
        }
        match analyze(&variant) {
            Ok(result) => {
                completed.push(name);
                for (base, count) in reference.cells.iter().zip(&mut counts) {
                    if let Some(cell) = result.cell_at(base.center) {
                        count.0 += usize::from(cell.score.is_some());
                        count.1 += usize::from(cell.selected);
                    }
                }
            }
            Err(error) => skipped.push(format!("{name}: {error}")),
        }
    }
    let n = completed.len();
    let cells = reference
        .cells
        .iter()
        .zip(counts)
        .map(|(cell, (supported, selected))| {
            let class = if supported == 0 {
                Class::Unsupported
            } else if n >= 5 && cell.selected && selected * 4 >= n * 3 {
                Class::StableLow
            } else if n >= 5 && supported == n && selected == 0 {
                Class::StableOther
            } else {
                Class::Uncertain
            };
            CellStability {
                center: cell.center,
                supported,
                selected,
                class,
            }
        })
        .collect();
    Sensitivity {
        completed,
        skipped,
        cells,
    }
}

/// Freeze contrasting candidate pairs before any independent observations are collected.
pub fn field_plan<'a>(
    project: &'a Project,
    analysis: &Analysis,
    stability: &Sensitivity,
) -> FieldPlan<'a> {
    let mut low = Vec::new();
    let mut high = Vec::new();
    let upper_threshold = analysis
        .score_range
        .map(|[min, max]| min + 0.5 * (max - min));
    for (cell, check) in analysis.cells.iter().zip(&stability.cells) {
        let Some(score) = cell.score else { continue };
        match check.class {
            Class::StableLow => low.push((cell.center, score)),
            Class::StableOther if upper_threshold.is_some_and(|t| score > t) => {
                high.push((cell.center, score))
            }
            _ => {}
        }
    }
    low.sort_by(|a, b| a.1.total_cmp(&b.1));
    high.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut used: Vec<Point> = Vec::new();
    let mut pairs = Vec::new();
    for (p, s) in low {
        if used.iter().any(|&point| point.distance(p) < 10.0) {
            continue;
        }
        if let Some(&(q, t)) = high.iter().find(|&&(q, t)| {
            t > s && p.distance(q) >= 10.0 && used.iter().all(|&point| point.distance(q) >= 10.0)
        }) {
            pairs.push(FieldPair {
                id: format!("P{:02}", pairs.len() + 1),
                lower: FieldPoint {
                    point: p,
                    modeled_score: s,
                },
                higher: FieldPoint {
                    point: q,
                    modeled_score: t,
                },
            });
            used.extend([p, q]);
        }
        if pairs.len() == 3 {
            break;
        }
    }
    FieldPlan {
        project_name: &project.name,
        coordinate_system: "Local metres, x east, y north",
        origin: project.origin,
        source_height_m: project.source_height_m,
        receiver_zones: &project.zones,
        receiver_sampling: "Use the same receiver per zone and compare both paired settings",
        pairs,
    }
}

pub fn color(class: Class) -> Option<[u8; 3]> {
    match class {
        Class::StableLow => Some([53, 218, 167]),
        Class::Uncertain => Some([244, 169, 74]),
        Class::StableOther => Some([81, 135, 169]),
        Class::Unsupported => None,
    }
}
