use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DataQuality {
    Ready,
    Real,
    Interpolated,
    Projected,
    Schematic,
    Missing,
    Stale,
}

impl Default for DataQuality {
    fn default() -> Self {
        Self::Missing
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MapMode {
    Gps,
    Projected,
    Schematic,
}

impl Default for MapMode {
    fn default() -> Self {
        Self::Schematic
    }
}
