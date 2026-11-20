# Silent Planner

Silent Planner is a native Rust desktop app for exploratory acoustic scenario mapping. Walkers renders the interactive basemap and handles panning, zooming and projection. The analysis engine runs locally with egui and eframe and requires neither Python nor CUDA.

## Run

From the repository root:

```bash
cargo run --locked --manifest-path planner/Cargo.toml --release
```

The Linux build uses X11 or Wayland and OpenGL. Native file and folder dialogs use the desktop portal on Linux. Rust 1.100.0 was used for verification. The demo uses the public map pin for Chungbuk Science High School in Cheongju. The three receiver zones and their sensitivity values are illustrative. The streets layer shows roads and labels by default. [School directions](https://school.cbe.go.kr/cbs-h/M010205/)

1. Under **Find your site**, enter a place name or decimal latitude and longitude, then select a result. Coordinate pairs are parsed locally. An explicit place-name search sends the typed query to the Photon public geocoder. Starting a different site after edits requires confirmation.
2. Click the map where people gather, or select **Add zone** if you are in pan mode. Click a zone to adjust its radius, name or sensitivity. Choose **Auto area** to fit around the zones or **Draw area** to drag a rectangle.
3. Switch between **Streets** and **Satellite** at the top of the map. Drag to pan and scroll to zoom, or use the zoom buttons and **Fit area**. Green marks the lowest-scoring fraction of eligible cells. The map stays visible around the colored areas. **Advanced settings** contains model and grid controls.
4. Select **Export map** to save in `planner-exports/`. The **Project** menu opens a saved project, changes the export folder or switches between light and dark appearance.

Use **Ctrl+K** on Windows or Linux and **Command+K** on macOS to focus place search. Press **Escape** to leave a map editing mode.

### Display font

DM Sans and JetBrains Mono ship with the desktop app. To use General Sans for headings, [download General Sans directly from Fontshare](https://www.fontshare.com/fonts/general-sans), extract `GeneralSans-Bold.otf`, and start the app with its path:

```bash
SILENT_PLANNER_DISPLAY_FONT="/path/to/GeneralSans-Bold.otf" cargo run --locked --manifest-path planner/Cargo.toml --release
```

## Files and automation

Each export creates its own folder with these files.

| File | Contents |
|---|---|
| `map.svg` | Standalone printable map and legend, including attributed satellite imagery when it was available |
| `analysis.json` | Inputs, model provenance, boundary and scored cells |
| `cells.csv` | Local metre coordinates, relative score and selection for every grid cell |
| `project.json` | Editable project file |
| `sensitivity.json` | Per-cell selection and support across radius, sensitivity and grid spacing variations |
| `field_plan.json` | Candidate position pairs for a separate field-ranking study |
| `impact.geojson` | WGS84 analysis boundary, occupied-zone centers and selected low-score cell polygons for projects with a geographic origin |

The scoring grid uses local metres, with positive x east and positive y north. A geographic origin anchors this local system to WGS84. The app uses a locally scaled Web Mercator projection for imagery alignment. An export with an origin can be opened as a GeoJSON layer in GIS software. It contains positions and labels you chose to include.

The **Streets** layer uses OpenStreetMap. **Satellite** uses the EOxCloudless 2016 mosaic, containing modified Copernicus Sentinel data from 2016–2017 under CC BY 4.0. Its approximately 10 m source resolution limits detail at building scale. The app reuses lower-zoom Sentinel tiles at close zoom and caches view tiles locally. With **Satellite** active, SVG export requests a separate attributed WMS raster. If that request fails, the SVG uses the grid background and includes an imagery status file in the bundle. Streets and licensed Korea HD layers provide viewing context. Their imagery is excluded from exports. Scoring and occupied-zone labels stay local.

For sites in South Korea with a permitted VWorld WMTS API key, launch with `SILENT_PLANNER_VWORLD_KEY` in the environment to enable **Korea HD**. The app uses the key only for VWorld tile requests. The regular layers work without it.

```bash
cargo run --locked --manifest-path planner/Cargo.toml --release -- --demo-export planner-exports
cargo run --locked --manifest-path planner/Cargo.toml --release -- --export /path/to/project.json planner-exports
cargo test --locked --manifest-path planner/Cargo.toml
cargo clippy --locked --manifest-path planner/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path planner/Cargo.toml -- --ignored
```

## Model and limits

The parent study selected the constant median. It supplies the reference energy level. A constant cannot rank locations, so the map applies the earlier exploratory linear fit to each candidate-to-receiver distance. The fit's fixed intercept and coefficients are exported with the dataset DOI. Its original test MAE was 4.866 dBFS and its test R-squared was -0.291. The large CUDA models were compared in the study and are separate artifacts.

Each occupied zone is represented by its center and eight evenly spaced points on its perimeter. The engine evaluates circular footprint distance limits, requires the closest and farthest points to fit the scene-specific horizontal distance range, and uses the nine points to estimate mean relative digital energy. Quiet outdoor cells require 5–70 m and road-adjacent cells 5–10 m. Source height is limited to 5–30 m. The guard checks observed marginal geometry ranges. It does not establish that every combination was measured.

Each predicted digital RMS is converted to an energy ratio using the study median as a reference. The map minimizes the largest sensitivity-weighted zone average. Green denotes ranking within the eligible map, and the threshold depends on the selected fraction. The output does not represent calibrated sound pressure, an indoor transmission model or an observed noise reduction. Building barriers, actual propagation and time-varying occupancy require separate field measurements. Use anonymous area labels.

See the [parent experiment results](../README.md#experimental-results) and the [third-party notices](../THIRD_PARTY_NOTICES.md).

Enable **Stability** alongside **Noise overlay** to display which cells remain low across the assumption variations. Green is consistently low, amber is uncertain, and blue is consistently unselected. Unscored cells are transparent. [Field verification protocol](../school_collection/field_rankings_protocol.md)
