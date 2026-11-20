// SPDX-License-Identifier: Apache-2.0
//! Basemap sources for the Walkers viewport.

use std::{env, path::PathBuf};

use silent_planner::satellite::{ATTRIBUTION, ATTRIBUTION_URL};
use walkers::{
    HttpOptions, HttpTiles, TileId,
    sources::{Attribution, TileSource},
};

pub const OSM_CREDIT: &str = "© OpenStreetMap contributors";
pub const OSM_URL: &str = "https://www.openstreetmap.org/copyright";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Basemap {
    #[default]
    Satellite,
    Streets,
    KoreaAerial,
}

pub fn vworld_key() -> Option<String> {
    env::var("SILENT_PLANNER_VWORLD_KEY").ok().filter(|key| {
        !key.is_empty() && key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    })
}

struct Source {
    kind: Basemap,
    vworld_key: String,
}

impl TileSource for Source {
    fn tile_url(&self, tile: TileId) -> String {
        let TileId { x, y, zoom } = tile;
        match self.kind {
            Basemap::Satellite => format!(
                "https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless_3857/default/g/{zoom}/{y}/{x}.jpg"
            ),
            Basemap::Streets => format!("https://tile.openstreetmap.org/{zoom}/{x}/{y}.png"),
            Basemap::KoreaAerial => format!(
                "https://api.vworld.kr/req/wmts/1.0.0/{}/Satellite/{zoom}/{y}/{x}.jpeg",
                self.vworld_key
            ),
        }
    }

    fn attribution(&self) -> Attribution {
        let (text, url) = attribution(self.kind);
        Attribution {
            text,
            url,
            logo_light: None,
            logo_dark: None,
        }
    }

    fn max_zoom(&self) -> u8 {
        match self.kind {
            // Sentinel-2 has no additional ground detail at street zoom.
            Basemap::Satellite => 14,
            Basemap::Streets | Basemap::KoreaAerial => 19,
        }
    }
}

pub fn attribution(kind: Basemap) -> (&'static str, &'static str) {
    match kind {
        Basemap::Satellite => (ATTRIBUTION, ATTRIBUTION_URL),
        Basemap::Streets => (OSM_CREDIT, OSM_URL),
        Basemap::KoreaAerial => (
            "© VWorld · Ministry of Land, Infrastructure and Transport",
            "https://www.vworld.kr/",
        ),
    }
}

pub fn tiles(kind: Basemap, key: Option<&str>, context: &eframe::egui::Context) -> HttpTiles {
    let cache = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")));
    let cache = cache.map(|path| path.join("silent-planner/tiles"));
    let options = HttpOptions {
        cache: if kind == Basemap::KoreaAerial {
            None
        } else {
            cache
        },
        user_agent: Some(walkers::HeaderValue::from_static(concat!(
            "SilentPlanner/",
            env!("CARGO_PKG_VERSION"),
            " (academic desktop map)"
        ))),
        ..Default::default()
    };
    HttpTiles::with_options(
        Source {
            kind,
            vworld_key: key.unwrap_or_default().into(),
        },
        options,
        context.clone(),
    )
}

/// Count visible tiles in the current frame without changing Walkers' cache.
pub struct VisibleTiles<'a> {
    inner: &'a mut HttpTiles,
    pub ready: usize,
}

impl<'a> VisibleTiles<'a> {
    pub fn new(inner: &'a mut HttpTiles) -> Self {
        Self { inner, ready: 0 }
    }
}

impl walkers::Tiles for VisibleTiles<'_> {
    fn at(&mut self, id: TileId) -> Option<walkers::TextureWithUv> {
        let tile = self.inner.at(id);
        self.ready += usize::from(tile.is_some());
        tile
    }

    fn attribution(&self) -> Attribution {
        self.inner.attribution()
    }

    fn tile_size(&self) -> u32 {
        self.inner.tile_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEOUL_TILE: TileId = TileId {
        x: 111767,
        y: 50757,
        zoom: 17,
    };

    #[test]
    fn source_tile_order_and_native_satellite_zoom() {
        let mut source = Source {
            kind: Basemap::Satellite,
            vworld_key: "public-test".into(),
        };
        assert!(
            source
                .tile_url(SEOUL_TILE)
                .ends_with("/17/50757/111767.jpg")
        );
        assert_eq!(source.max_zoom(), 14);
        source.kind = Basemap::Streets;
        assert!(
            source
                .tile_url(SEOUL_TILE)
                .ends_with("/17/111767/50757.png")
        );
        source.kind = Basemap::KoreaAerial;
        assert!(
            source
                .tile_url(SEOUL_TILE)
                .ends_with("/public-test/Satellite/17/50757/111767.jpeg")
        );
    }

    #[test]
    #[ignore = "Requires access to public map layers"]
    fn downloads_and_decodes_live_tiles() {
        use walkers::Tiles;

        let ctx = eframe::egui::Context::default();
        for kind in [Basemap::Satellite, Basemap::Streets] {
            let mut tiles = tiles(kind, None, &ctx);
            assert!(
                (0..100).any(|_| {
                    let ready = tiles.at(SEOUL_TILE).is_some();
                    if !ready {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    ready
                }),
                "Walkers did not return a tile for {kind:?}"
            );
        }
    }
}
