// SPDX-License-Identifier: Apache-2.0
//! Verify map scoring, numerical support and editable export round trips.

use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use silent_planner::{
    export::{export, open_project},
    map::{Bounds, Point, Project, Scene, Zone, analyze, predict_linear},
    satellite::Origin,
    sensitivity::{Class, assess, field_plan},
};

#[test]
fn sensitivity_uses_reference_grid_and_freezes_contrasting_pairs() {
    let project = Project::demo();
    let reference = analyze(&project).unwrap();
    let result = assess(&project, &reference);
    assert_eq!(result.completed.len(), 7);
    assert!(result.skipped.is_empty());
    assert_eq!(result.cells.len(), reference.cells.len());
    for (cell, stability) in reference.cells.iter().zip(&result.cells) {
        assert_eq!(cell.center, stability.center);
        assert!(stability.selected <= stability.supported);
        assert!(stability.supported <= result.completed.len());
        if stability.class == Class::StableLow {
            assert!(cell.selected);
            assert!(stability.selected * 4 >= result.completed.len() * 3);
        }
    }
    let plan = field_plan(&project, &reference, &result);
    assert!(!plan.pairs.is_empty());
    for pair in plan.pairs {
        assert!(pair.lower.modeled_score < pair.higher.modeled_score);
        assert!(pair.lower.point.distance(pair.higher.point) >= 10.0);
    }
}

#[test]
fn georeference_preserves_origin_and_local_orientation() {
    let origin = Origin::EXAMPLE;
    let [longitude, latitude] = origin.lon_lat(Point::default());
    assert!((longitude - origin.longitude).abs() < 1e-10);
    assert!((latitude - origin.latitude).abs() < 1e-10);
    assert!(origin.lon_lat(Point::new(10.0, 0.0))[0] > longitude);
    assert!(origin.lon_lat(Point::new(0.0, 10.0))[1] > latitude);
    assert!(
        !Origin {
            latitude: 90.0,
            longitude: 20.0
        }
        .valid()
    );
}

#[test]
fn demo_has_ranked_cells_and_uses_its_automatic_envelope() {
    let project = Project::demo();
    assert_eq!(project.origin, Some(Origin::CHUNGBUK_SCIENCE_HIGH_SCHOOL));
    assert!(
        project
            .zones
            .iter()
            .all(|zone| zone.label.starts_with("Example "))
    );
    let analysis = analyze(&project).unwrap();
    assert_eq!(analysis.bounds, project.auto_bounds());
    assert!(analysis.scored_count > analysis.selected_count);
    assert!(analysis.selected_count > 0);
    assert_eq!(
        analysis.scored_count,
        analysis
            .cells
            .iter()
            .filter(|cell| cell.score.is_some())
            .count()
    );
    let selected_max = analysis
        .cells
        .iter()
        .filter(|cell| cell.selected)
        .filter_map(|cell| cell.score)
        .reduce(f64::max)
        .unwrap();
    let unselected_min = analysis
        .cells
        .iter()
        .filter(|cell| !cell.selected)
        .filter_map(|cell| cell.score)
        .reduce(f64::min)
        .unwrap();
    assert!(selected_max <= unselected_min);
}

#[test]
fn manual_bounds_and_distance_support_are_applied() {
    let bounds = Bounds::spanning(Point::new(0.0, 0.0), Point::new(30.0, 2.0));
    let project = Project {
        zones: vec![Zone {
            label: "Receiver".into(),
            center: Point::new(0.0, 1.0),
            radius_m: 1.0,
            weight: 1.0,
        }],
        bounds: Some(bounds),
        scene: Scene::Roadside,
        ..Project::default()
    };
    let analysis = analyze(&project).unwrap();
    assert_eq!(analysis.bounds, bounds);
    assert!(
        analysis
            .cell_at(Point::new(3.0, 1.0))
            .unwrap()
            .score
            .is_none()
    );
    assert!(
        analysis
            .cell_at(Point::new(7.0, 1.0))
            .unwrap()
            .score
            .is_some()
    );
    assert!(
        analysis
            .cell_at(Point::new(13.0, 1.0))
            .unwrap()
            .score
            .is_none()
    );
    assert!(analysis.cell_at(Point::new(30.1, 1.0)).is_none());
}

#[test]
fn empty_project_emits_no_scores() {
    let result = analyze(&Project::default()).unwrap();
    assert_eq!(result.scored_count, 0);
    assert_eq!(result.selected_count, 0);
    assert!(result.score_range.is_none());
}

#[test]
fn prediction_matches_frozen_equation_and_masks_extrapolation() {
    let level = predict_linear(20.0, 10.0, Scene::Quiet).unwrap();
    assert!((level - -48.74226191007078).abs() < 1e-11);
    assert!(predict_linear(20.0, 10.0, Scene::Roadside).is_none());
    assert!(predict_linear(4.99, 15.0, Scene::Quiet).is_none());
    assert!(predict_linear(20.0, 30.01, Scene::Quiet).is_none());
}

#[test]
fn invalid_and_excessive_grids_report_errors() {
    let mut project = Project {
        source_height_m: f64::NAN,
        ..Project::demo()
    };
    assert!(analyze(&project).is_err());
    project.source_height_m = 15.0;
    project.cell_size_m = 0.5;
    project.bounds = Some(Bounds::spanning(
        Point::default(),
        Point::new(1000.0, 1000.0),
    ));
    assert!(analyze(&project).unwrap_err().contains("grid exceeds"));
}

#[test]
fn export_round_trip_preserves_edits_and_escapes_svg_text() {
    let directory = std::env::temp_dir().join(format!(
        "silent-planner-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut project = Project::demo();
    project.name = "Site <&> \"A\"".into();
    let output = export(&project, &directory).unwrap();
    assert_eq!(open_project(&output.join("project.json")).unwrap(), project);
    let svg = fs::read_to_string(output.join("map.svg")).unwrap();
    assert!(svg.contains("Site &lt;&amp;&gt; &quot;A&quot;"));
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("analysis.json")).unwrap()).unwrap();
    let csv = fs::read_to_string(output.join("cells.csv")).unwrap();
    assert_eq!(
        csv.lines().count(),
        json["analysis"]["cells"].as_array().unwrap().len() + 1
    );
    let geographic: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("impact.geojson")).unwrap()).unwrap();
    let features = geographic["features"].as_array().unwrap();
    assert_eq!(geographic["type"], "FeatureCollection");
    assert_eq!(
        features.len(),
        1 + project.zones.len() + analyze(&project).unwrap().selected_count
    );
    let ring = features[0]["geometry"]["coordinates"][0]
        .as_array()
        .unwrap();
    assert_eq!(ring.first(), ring.last());
    fs::remove_dir_all(directory).unwrap();
}
