use std::collections::HashMap;

pub mod ihp_sg13g2;

// Algorithm parameter structs

/// Parameters for the square (checkerboard) fill algorithm.
pub struct SquareParams {
    pub min_width: f64,
    pub max_width: f64,
    pub min_space: f64,
    pub max_space: f64,
    /// Whether fill squares may be clipped at tile edges (default true).
    pub clipping: bool,
}

/// Orientation of fill tracks (rows vs columns).
pub enum TrackOrientation {
    Horizontal,
    Vertical,
}

/// Parameters for the track-based fill algorithm.
///
/// Tracks are parallel stripes whose pitch is determined by `min_width`,
/// `min_space`, and `gaps` (the routing track pitch to align to).
pub struct TrackParams {
    pub min_width: f64,
    pub max_width: f64,
    pub min_space: f64,
    pub max_space: f64,
    pub orientation: TrackOrientation,
    /// Routing track pitch to snap fill stripes to (in µm).
    pub gaps: f64,
    /// Standard-cell row height used to align fill (in µm).
    pub cell_height: f64,
    /// When `true`, allow fill up to the tile edge even if `max_space` is exceeded.
    pub aggressive_fill: bool,
    /// Fractions of `n_max = floor(max_width / gaps)` that determine the
    /// perpendicular fill sizes (across routing tracks).  Each entry `f` produces
    /// size `floor(f * n_max) * gaps`, snapped to the manufacturing grid.
    /// `min_width` is always appended as the final perpendicular size.
    /// Example: `&[1.0, 0.6, 0.5, 0.4, 0.3]` for M2/M4 (gaps=0.48 µm, max=5 µm).
    pub pass_fracs: &'static [f64],
    /// Explicit free-direction sizes in µm (along the routing direction).
    /// Tried from largest to smallest; each is combined with every perpendicular
    /// size from `pass_fracs`.  Values are snapped to the manufacturing grid
    /// and clamped to `[min_width, max_width]`.
    /// Example: `&[5.0, 4.0, 3.0, 2.0, 1.5, 1.0]`.
    pub free_heights_um: &'static [f64],
}

/// Parameters for the overlap fill algorithm used on GatPoly.
///
/// Fill rectangles extend `min_extension` beyond existing geometry to keep
/// gate-poly fill electrically equivalent to drawn gates.
pub struct OverlapParams {
    pub min_width: f64,
    pub max_width: f64,
    /// Minimum extension of fill past existing geometry (in µm).
    pub min_extension: f64,
    pub min_space: f64,
    /// Name of the PDK layer whose placed fill shapes are used as placement
    /// anchors (e.g. `"Activ"` for GatPoly overlap fill).
    pub ref_layer: &'static str,
}

// Algorithm enum

/// Fill strategy for a single PDK layer.
///
/// Multiple algorithms may be listed in [`PdkLayer::algorithms`]; they are
/// applied in order, each operating on the space left by the previous pass.
pub enum FillAlgorithm {
    Square(SquareParams),
    Track(TrackParams),
    Overlap(OverlapParams),
}

fn square(min_width: f64, max_width: f64, min_space: f64, max_space: f64) -> FillAlgorithm {
    FillAlgorithm::Square(SquareParams { min_width, max_width, min_space, max_space, clipping: true })
}

fn square_noclip(min_width: f64, max_width: f64, min_space: f64, max_space: f64) -> FillAlgorithm {
    FillAlgorithm::Square(SquareParams { min_width, max_width, min_space, max_space, clipping: false })
}

#[allow(clippy::too_many_arguments)]
fn track_v(
    min_width: f64, max_width: f64,
    min_space: f64, max_space: f64,
    gaps: f64, cell_height: f64,
    pass_fracs: &'static [f64],
    free_heights_um: &'static [f64],
) -> FillAlgorithm {
    FillAlgorithm::Track(TrackParams {
        min_width, max_width, min_space, max_space,
        orientation: TrackOrientation::Vertical,
        gaps, cell_height,
        aggressive_fill: false,
        pass_fracs,
        free_heights_um,
    })
}

#[allow(clippy::too_many_arguments)]
fn track_h(
    min_width: f64, max_width: f64,
    min_space: f64, max_space: f64,
    gaps: f64, cell_height: f64,
    pass_fracs: &'static [f64],
    free_heights_um: &'static [f64],
) -> FillAlgorithm {
    FillAlgorithm::Track(TrackParams {
        min_width, max_width, min_space, max_space,
        orientation: TrackOrientation::Horizontal,
        gaps, cell_height,
        aggressive_fill: false,
        pass_fracs,
        free_heights_um,
    })
}

fn overlap(min_width: f64, max_width: f64, min_extension: f64, min_space: f64) -> FillAlgorithm {
    FillAlgorithm::Overlap(OverlapParams { min_width, max_width, min_extension, min_space, ref_layer: "Activ" })
}

// Layer and process constants

/// PDK-level description of a single physical layer.
pub struct PdkLayer {
    /// GDS layer number.
    pub gds_layer: i16,
    /// GDS datatype for drawing shapes (typically 0).
    pub drawing_datatype: i16,
    /// GDS datatype written for generated fill shapes.
    pub fill_datatype: i16,
    /// GDS datatype of no-fill keep-out markers.
    pub nofill_datatype: i16,
    /// Maximum cell hierarchy depth traversed when collecting shapes.
    pub max_depth: u32,
    /// Fill algorithms applied in order (e.g. Track first, then Square for remainder).
    pub algorithms: Vec<FillAlgorithm>,
    /// Default target fill density in percent.
    pub default_density: f64,
    /// Default acceptable deviation from the target in percent.
    pub default_deviation: f64,
    /// Tile width used during fill for this layer, in micrometres.
    pub tile_width_um: f64,
    /// Merge drawing polygons before density calculation.
    /// Required for layers where IO-filler cells produce intentionally overlapping
    /// or self-touching shapes (Metal3 and above).
    pub merge_for_density: bool,
    /// Maximum merge window size in µm used by the tiled merge in density
    /// calculation.  `None` means the density tile itself is the window (safe
    /// for sparse layers like TopMetal).  Set to a small value (e.g. 50 µm)
    /// for dense layers (Activ, GatPoly, Metal1) to bound peak memory.
    pub merge_window_um: Option<f64>,
}

/// Process-wide constants: layer table and global geometry parameters.
pub struct PdkConstants {
    /// All fillable layers keyed by their canonical name (e.g. `"Metal1"`).
    pub layers: HashMap<&'static str, PdkLayer>,
    /// Size of one database unit in micrometres (0.001 for IHP = 1 DBU -> 1 nm).
    pub db_unit_um: f64,
    /// Default tile width in micrometres.
    pub tile_width_um: f64,
    /// GDS (layer, datatype) that defines the chip boundary (Edge.Seal for IHP).
    pub boundary_layer: Option<(i16, i16)>,
    /// Manufacturing grid in database units (e.g. 5 DBU = 5 nm for IHP).
    /// Fill shape sizes and spaces are snapped to multiples of `2 * grid_dbu`
    /// so that `half = size/2` always lands on a grid point.
    pub grid_dbu: f64,
}

impl PdkConstants {
    /// Return constants for the named process, or `None` if unknown.
    pub fn for_process(process: &str) -> Option<Self> {
        match process {
            "ihp-sg13g2"     => Some(ihp_sg13g2()),
            "ihp-sg13cmos5l" => Some(ihp_sg13cmos5l()),
            _ => None,
        }
    }

    /// Tile width converted to database units.
    pub fn tile_width_dbu(&self) -> f64 {
        self.tile_width_um / self.db_unit_um
    }
}

macro_rules! ihp_layer {
    ($gds_layer:expr, [$($alg:expr),+ $(,)?], $density:expr, $deviation:expr, $tile_um:expr) => {
        PdkLayer {
            gds_layer:         $gds_layer,
            drawing_datatype:  0,
            fill_datatype:     22,
            nofill_datatype:   23,
            max_depth:         10,
            algorithms:        vec![$($alg),+],
            default_density:   $density,
            default_deviation: $deviation,
            tile_width_um:     $tile_um,
            merge_for_density: true,
            merge_window_um:   None,
        }
    };
    // merge = <window_um>: tiled merge with the given sub-window size.
    // Use for dense layers with many small structures (Activ, GatPoly, Metal1).
    ($gds_layer:expr, [$($alg:expr),+ $(,)?], $density:expr, $deviation:expr, $tile_um:expr, merge = $window:expr) => {
        PdkLayer {
            merge_window_um:   Some($window),
            ..ihp_layer!($gds_layer, [$($alg),+], $density, $deviation, $tile_um)
        }
    };
}

// IHP SG13G2

#[rustfmt::skip]
fn ihp_sg13g2() -> PdkConstants {
    let mut layers = HashMap::new();

    layers.insert("Activ", ihp_layer!(
        1, [square_noclip(1.08, 4.63, 1.8, 10.0)],
        50.0, 5.0, 100.0, merge = 50.0));

    layers.insert("GatPoly", ihp_layer!(
        5, [overlap(0.7, 5.0, 0.18, 0.8)],
        25.0, 3.0, 100.0, merge = 50.0));

    layers.insert("Metal1", ihp_layer!(
        8, [square(1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0, merge = 50.0));

    layers.insert("Metal2", ihp_layer!(
        10, [track_v(1.0, 5.0, 0.42, 10.0, 0.48, 1.44,
                     &[1.0, 0.6, 0.5, 0.4, 0.3], &[5.0, 4.0, 3.0, 2.0, 1.5, 1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0));

    layers.insert("Metal3", ihp_layer!(
        30, [track_h(1.0, 5.0, 0.42, 10.0, 0.42, 1.26,
                     &[0.3], &[1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0));

    layers.insert("Metal4", ihp_layer!(
        50, [track_v(1.0, 5.0, 0.42, 10.0, 0.48, 1.44,
                     &[0.3], &[1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0));

    layers.insert("Metal5", ihp_layer!(
        67, [track_h(1.0, 5.0, 0.42, 10.0, 0.42, 1.26,
                     &[0.3], &[1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 100.0));

    layers.insert("TopMetal1", ihp_layer!(
        126, [square(5.0, 10.0, 3.0, 10.0)],
        40.0, 10.0, 800.0));

    layers.insert("TopMetal2", ihp_layer!(
        134, [square(5.0, 10.0, 3.0, 10.0)],
        40.0, 10.0, 800.0));

    PdkConstants {
        layers, db_unit_um: 0.001, tile_width_um: 800.0, boundary_layer: Some((39, 0)),
        grid_dbu: 5.0,
    }
}

// IHP SG13CMOS5L

#[rustfmt::skip]
fn ihp_sg13cmos5l() -> PdkConstants {
    let mut layers = HashMap::new();

    layers.insert("Activ", ihp_layer!(
        1, [square_noclip(1.08, 4.63, 1.8, 10.0)],
        50.0, 5.0, 400.0, merge = 50.0));

    layers.insert("GatPoly", ihp_layer!(
        5, [overlap(0.7, 5.0, 0.18, 0.8)],
        25.0, 2.0, 400.0, merge = 50.0));

    layers.insert("Metal1", ihp_layer!(
        8, [square(1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 400.0, merge = 50.0));

    layers.insert("Metal2", ihp_layer!(
        10, [track_v(1.0, 5.0, 0.42, 10.0, 0.48, 1.44,
                     &[1.0, 0.6, 0.5, 0.4, 0.3], &[5.0, 4.0, 3.0, 2.0, 1.5, 1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 400.0));

    layers.insert("Metal3", ihp_layer!(
        30, [track_h(1.0, 5.0, 0.42, 10.0, 0.42, 1.26,
                     &[0.3], &[1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 400.0));

    layers.insert("Metal4", ihp_layer!(
        50, [track_v(1.0, 5.0, 0.42, 10.0, 0.48, 1.44,
                     &[0.3], &[1.0]),
             square( 1.0, 5.0, 0.42, 10.0)],
        50.0, 10.0, 400.0));

    layers.insert("TopMetal1", ihp_layer!(
        126, [square(5.0, 10.0, 3.0, 10.0)],
        40.0, 10.0, 800.0));

    PdkConstants {
        layers, db_unit_um: 0.001, tile_width_um: 800.0, boundary_layer: Some((39, 0)),
        grid_dbu: 5.0,
    }
}
