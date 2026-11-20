// SPDX-License-Identifier: Apache-2.0
//! Georeference local maps and fetch a raster for standalone SVG exports.

use std::{
    io::{Cursor, Read},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::map::{Bounds, Point};

const RADIUS_M: f64 = 6_378_137.0;
const ENDPOINT: &str = "https://tiles.maps.eox.at/wms";
pub const ATTRIBUTION: &str = "EOxCloudless by EOX IT Services GmbH (Contains modified Copernicus Sentinel data 2016 & 2017), CC BY 4.0";
pub const ATTRIBUTION_URL: &str = "https://cloudless.eox.at/license-non-commercial";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Origin {
    pub latitude: f64,
    pub longitude: f64,
}

impl Origin {
    pub const EXAMPLE: Self = Self {
        latitude: 37.5665,
        longitude: 126.9780,
    };

    pub fn valid(self) -> bool {
        (-80.0..=80.0).contains(&self.latitude) && (-179.0..=179.0).contains(&self.longitude)
    }

    fn projected(self, local: Point) -> Point {
        let latitude = self.latitude.to_radians();
        let multiplier = 1.0 / latitude.cos();
        Point::new(
            RADIUS_M * self.longitude.to_radians() + local.x * multiplier,
            RADIUS_M * latitude.tan().asinh() + local.y * multiplier,
        )
    }

    /// Convert local east and north metres to GeoJSON longitude and latitude.
    pub fn lon_lat(self, local: Point) -> [f64; 2] {
        let world = self.projected(local);
        [
            (world.x / RADIUS_M).to_degrees(),
            (world.y / RADIUS_M).sinh().atan().to_degrees(),
        ]
    }
}

pub fn fetch(origin: Origin, bounds: Bounds) -> Result<Vec<u8>, String> {
    if !origin.valid() || bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return Err("Invalid satellite viewport.".into());
    }
    let min = origin.projected(bounds.min);
    let max = origin.projected(bounds.max);
    let bbox = format!("{:.3},{:.3},{:.3},{:.3}", min.x, min.y, max.x, max.y);
    let scale = 1100.0 / bounds.width().max(bounds.height());
    let width = (bounds.width() * scale).round().max(1.0) as u32;
    let height = (bounds.height() * scale).round().max(1.0) as u32;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(12))
        .try_proxy_from_env(true)
        .build();
    let response = agent
        .get(ENDPOINT)
        .set(
            "User-Agent",
            concat!("SilentPlanner/", env!("CARGO_PKG_VERSION")),
        )
        .query("SERVICE", "WMS")
        .query("VERSION", "1.1.1")
        .query("REQUEST", "GetMap")
        .query("LAYERS", "s2cloudless_3857")
        .query("STYLES", "")
        .query("SRS", "EPSG:3857")
        .query("BBOX", &bbox)
        .query("WIDTH", &width.to_string())
        .query("HEIGHT", &height.to_string())
        .query("FORMAT", "image/jpeg")
        .call()
        .map_err(|error| format!("Satellite service unavailable: {error}"))?;
    let mut jpeg = Vec::new();
    response
        .into_reader()
        .take(8_000_001)
        .read_to_end(&mut jpeg)
        .map_err(|error| format!("Unable to read satellite image: {error}"))?;
    if jpeg.len() > 8_000_000 {
        return Err("Satellite response exceeds the image limit.".into());
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(&jpeg), image::ImageFormat::Jpeg);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1100);
    limits.max_image_height = Some(1100);
    reader.limits(limits);
    let pixels = reader
        .decode()
        .map_err(|error| format!("Invalid satellite image response: {error}"))?;
    if pixels.width() != width || pixels.height() != height {
        return Err("Satellite image size did not match the requested viewport.".into());
    }
    Ok(jpeg)
}
