// SPDX-License-Identifier: Apache-2.0
//! Resolve a submitted place query, or parse coordinates locally.

use std::{io::Read, time::Duration};

use serde::Deserialize;
use silent_planner::satellite::Origin;

#[derive(Clone, Debug)]
pub struct Place {
    pub label: String,
    pub origin: Origin,
}

pub fn coordinates(query: &str) -> Option<Origin> {
    let mut fields = query
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|x| !x.is_empty());
    let origin = Origin {
        latitude: fields.next()?.parse().ok()?,
        longitude: fields.next()?.parse().ok()?,
    };
    (fields.next().is_none() && origin.valid()).then_some(origin)
}

#[derive(Deserialize)]
struct Response {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    geometry: Option<Geometry>,
    properties: Properties,
}

#[derive(Deserialize)]
struct Geometry {
    coordinates: [f64; 2],
}

#[derive(Deserialize)]
struct Properties {
    name: Option<String>,
    street: Option<String>,
    city: Option<String>,
    state: Option<String>,
    country: Option<String>,
}

fn decode(bytes: &[u8]) -> Result<Vec<Place>, String> {
    let data: Response = serde_json::from_slice(bytes)
        .map_err(|_| "The place service returned an invalid response.".to_string())?;
    let places = data
        .features
        .into_iter()
        .filter_map(|feature| {
            let [longitude, latitude] = feature.geometry?.coordinates;
            let origin = Origin {
                latitude,
                longitude,
            };
            if !origin.valid() {
                return None;
            }
            let fields = feature.properties;
            let mut label = Vec::new();
            for text in [
                fields.name.or(fields.street),
                fields.city,
                fields.state,
                fields.country,
            ]
            .into_iter()
            .flatten()
            {
                if !label.contains(&text) {
                    label.push(text);
                }
            }
            Some(Place {
                label: label.join(", "),
                origin,
            })
        })
        .filter(|place| !place.label.is_empty())
        .take(5)
        .collect::<Vec<_>>();
    if places.is_empty() {
        return Err("No places found. Try adding a city or enter latitude, longitude.".into());
    }
    Ok(places)
}

pub fn search(query: &str) -> Result<Vec<Place>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
        .try_proxy_from_env(true)
        .build();
    let response = agent
        .get("https://photon.komoot.io/api/")
        .set(
            "User-Agent",
            concat!("SilentPlanner/", env!("CARGO_PKG_VERSION")),
        )
        .query("q", query)
        .query("limit", "5")
        .call()
        .map_err(|_| {
            "Place search is unavailable. You can still enter latitude, longitude.".to_string()
        })?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(262_145)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read the place search response.".to_string())?;
    if bytes.len() > 262_144 {
        return Err("The place search response was too large.".into());
    }
    decode(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_coordinates_without_network() {
        assert_eq!(coordinates("37.5665, 126.978"), Some(Origin::EXAMPLE));
        assert_eq!(coordinates("37.5665 126.978"), Some(Origin::EXAMPLE));
        assert!(coordinates("Seoul, South Korea").is_none());
        assert!(coordinates("97, 126").is_none());
    }

    #[test]
    fn converts_search_results_to_latitude_longitude() {
        let matches = decode(br#"{"features":[{"geometry":{"coordinates":[126.978,37.5665]},"properties":{"name":"City Hall","city":"Seoul"}}]}"#).unwrap();
        assert_eq!(matches[0].origin, Origin::EXAMPLE);
        assert_eq!(matches[0].label, "City Hall, Seoul");
    }

    #[test]
    #[ignore = "Requires access to the public Photon service"]
    fn live_place_search() {
        let places = search("Seoul City Hall").unwrap();
        assert!(
            places
                .iter()
                .any(|place| (place.origin.latitude - 37.5665).abs() < 0.03)
        );
    }
}
