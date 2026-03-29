pub mod config;
pub mod density;
pub mod erase;
pub mod fill;
pub mod pdk;

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
use geo::{Area, BooleanOps, BoundingRect, coord, LineString, Polygon, Rect};
use gds21::{GdsArrayRef, GdsElement, GdsLibrary, GdsPath, GdsStrans, GdsStruct, GdsStructRef};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::float::simplify::SimplifyShape;
use gzp::{deflate::Gzip, par::compress::{ParCompress, ParCompressBuilder}, ZWriter};
use rayon::prelude::*;

use config::FillConfig;
use pdk::{PdkConstants, PdkLayer};

// Debug datatype constants

/// Datatype used to write keep-out polygons for visual debugging.
pub const DEBUG_KEEPOUT_DT: i16 = 250;
/// Datatype used to write merged drawing + fill polygons for visual debugging.
pub const DEBUG_MERGED_DT: i16 = 251;

// Shared run context

pub struct RunContext {
    pub process: String,
    pub config: Option<FillConfig>,
    pub pdk: PdkConstants,
}

impl RunContext {
    pub fn new(process: &str, config_path: Option<&Path>) -> Result<Self> {
        let config = config_path
            .map(FillConfig::from_file)
            .transpose()
            .with_context(|| {
                format!("Failed to read config file: {}", config_path.unwrap().display())
            })?;
        validate_process(process, config.as_ref())?;
        let pdk = PdkConstants::for_process(process).ok_or_else(|| {
            anyhow!("Unknown process '{}'. Supported: ihp-sg13g2, ihp-sg13cmos5l", process)
        })?;
        Ok(Self { process: process.to_owned(), config, pdk })
    }
}

// Error handling

pub fn gds_err(e: gds21::GdsError) -> anyhow::Error {
    anyhow!("{e}")
}

// Process validation

pub fn validate_process(process: &str, config: Option<&FillConfig>) -> Result<()> {
    if let Some(config_pdk) = config.and_then(|c| c.pdk.as_deref())
        && config_pdk != process {
            return Err(anyhow!(
                "Process mismatch: --process is '{}' but config specifies PDK '{}'",
                process, config_pdk
            ));
        }
    Ok(())
}

// GDS I/O

pub fn is_gzip(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "gz")
}

pub fn read_gds(path: &Path) -> Result<GdsLibrary> {
    let is_gz = is_gzip(path);
    let bytes = if is_gz {
        let file = std::fs::File::open(path)?;
        let mut decoder = GzDecoder::new(file);
        let mut bytes = Vec::new();
        decoder.read_to_end(&mut bytes)?;
        bytes
    } else {
        std::fs::read(path)?
    };
    GdsLibrary::from_bytes(&bytes).map_err(gds_err)
}

pub fn write_gds(lib: &GdsLibrary, path: &Path) -> Result<()> {
    let is_gz = is_gzip(path);
    let file = std::fs::File::create(path)?;
    if is_gz {
        let mut encoder: ParCompress<Gzip, _> = ParCompressBuilder::new().from_writer(file);
        lib.write(&mut encoder).map_err(gds_err)?;
        encoder.finish().map_err(|e| anyhow!("{e}"))?;
    } else {
        lib.save(path).map_err(gds_err)?;
    }
    Ok(())
}

pub fn is_fill_element(
    elem: &GdsElement,
    targets: &std::collections::HashSet<(i16, i16)>,
) -> bool {
    match elem {
        GdsElement::GdsBoundary(b) => targets.contains(&(b.layer, b.datatype)),
        GdsElement::GdsPath(p) => targets.contains(&(p.layer, p.datatype)),
        _ => false,
    }
}

pub fn get_target_layers(ctx: &RunContext) -> Vec<(&str, &PdkLayer)> {
    let RunContext { process, config, pdk } = ctx;

    let layer_filter: Vec<String> = config.as_ref().map(|c| c.layer_names()).unwrap_or_default();
    let mut layers: Vec<(&str, &PdkLayer)> = if layer_filter.is_empty() {
        pdk.layers.iter().map(|(name, l)| (*name, l)).collect()
    } else {
        let mut targets = Vec::new();
        for name in &layer_filter {
            match pdk.layers.get_key_value(name.as_str()) {
                Some((key, l)) => targets.push((*key, l)),
                None => eprintln!("Warning: layer '{}' not found in PDK '{}', skipping", name, process),
            }
        }
        targets
    };
    layers.sort_by_key(|&(_, layer)| layer.gds_layer);
    layers
}

// Affine transform

/// 2-D affine transform: applies linear map then translation.
///
/// ```text
/// [x']   [a  b] [x]   [tx]
/// [y'] = [c  d] [y] + [ty]
/// ```
#[derive(Clone)]
pub(crate) struct AffineTransform {
    pub(crate) a: f64, pub(crate) b: f64,
    pub(crate) c: f64, pub(crate) d: f64,
    pub(crate) tx: f64, pub(crate) ty: f64,
}

impl AffineTransform {
    pub(crate) fn identity() -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 }
    }

    /// Build the transform for a GDS placement (SRef or one ARef instance).
    ///
    /// GDS applies transforms in this order:
    ///   1. optional x-axis reflection
    ///   2. rotation by `angle` degrees
    ///   3. magnification
    ///   4. translation to `(tx, ty)`
    pub(crate) fn from_strans_xy(strans: Option<&GdsStrans>, tx: f64, ty: f64) -> Self {
        let mag       = strans.and_then(|s| s.mag).unwrap_or(1.0);
        let angle_deg = strans.and_then(|s| s.angle).unwrap_or(0.0);
        let reflected = strans.is_some_and(|s| s.reflected);

        let angle  = angle_deg.to_radians();
        let cos_a  = angle.cos() * mag;
        let sin_a  = angle.sin() * mag;

        // Without reflection: standard rotation matrix.
        // With reflection: reflect about x-axis first (y -> -y), then rotate.
        let (a, b, c, d) = if reflected {
            ( cos_a,  sin_a,
              sin_a, -cos_a)
        } else {
            ( cos_a, -sin_a,
              sin_a,  cos_a)
        };

        Self { a, b, c, d, tx, ty }
    }

    pub(crate) fn from_sref(r: &GdsStructRef) -> Self {
        Self::from_strans_xy(r.strans.as_ref(), r.xy.x as f64, r.xy.y as f64)
    }

    /// Apply this transform to a point.
    pub(crate) fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.b * y + self.tx,
         self.c * x + self.d * y + self.ty)
    }

    /// Compose: `self` applied after `inner`  (i.e. `self composed with inner`).
    pub(crate) fn compose(&self, inner: &Self) -> Self {
        Self {
            a:  self.a * inner.a + self.b * inner.c,
            b:  self.a * inner.b + self.b * inner.d,
            c:  self.c * inner.a + self.d * inner.c,
            d:  self.c * inner.b + self.d * inner.d,
            tx: self.a * inner.tx + self.b * inner.ty + self.tx,
            ty: self.c * inner.tx + self.d * inner.ty + self.ty,
        }
    }
}

// Layer map

/// All flattened polygons indexed by `(layer, datatype)`.
pub struct LayerMap {
    inner: HashMap<(i16, i16), Vec<Polygon<f64>>>,
}

impl LayerMap {
    /// Traverse the full cell hierarchy and collect every boundary polygon.
    /// All placement transforms are applied so coordinates are in the top-level frame.
    pub fn build(lib: &GdsLibrary) -> Self {
        Self::build_for(lib, None)
    }

    /// Like `build` but only retains polygons on `(layer, datatype)` pairs present
    /// in `needed`.  Reduces peak memory when only a few layers are required.
    /// Pass `None` to collect everything.
    pub fn build_for(lib: &GdsLibrary, needed: Option<&HashSet<(i16, i16)>>) -> Self {
        let struct_map: HashMap<&str, &GdsStruct> =
            lib.structs.iter().map(|s| (s.name.as_str(), s)).collect();

        let referenced: HashSet<&str> = lib.structs.iter()
            .flat_map(|s| s.elems.iter())
            .filter_map(|e| match e {
                GdsElement::GdsStructRef(r) => Some(r.name.as_str()),
                GdsElement::GdsArrayRef(r)  => Some(r.name.as_str()),
                _ => None,
            })
            .collect();

        let mut inner: HashMap<(i16, i16), Vec<Polygon<f64>>> = HashMap::new();
        for top in lib.structs.iter().filter(|s| !referenced.contains(s.name.as_str())) {
            collect_all_recursive(top, &struct_map, needed, &AffineTransform::identity(), &mut inner);
        }

        // Deduplicate exactly identical polygons (same integer coordinates).
        // Happens when standard cells have shapes duplicated in their definition.
        for polys in inner.values_mut() {
            let mut seen: HashSet<Vec<[i64; 2]>> = HashSet::new();
            polys.retain(|p| {
                let key: Vec<[i64; 2]> = p.exterior().0.iter()
                    .map(|c| [c.x.round() as i64, c.y.round() as i64])
                    .collect();
                seen.insert(key)
            });
        }

        Self { inner }
    }

    /// O(1) lookup -- returns an empty slice when the layer/datatype is absent.
    pub fn polygons(&self, layer: i16, datatype: i16) -> &[Polygon<f64>] {
        self.inner.get(&(layer, datatype)).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Compute the bounding box of all shapes on `(layer, datatype)`.
    pub fn bbox(&self, layer: i16, datatype: i16) -> Option<Rect<f64>> {
        let polys = self.polygons(layer, datatype);
        if polys.is_empty() {
            return None;
        }
        let mut x_min = f64::INFINITY;
        let mut y_min = f64::INFINITY;
        let mut x_max = f64::NEG_INFINITY;
        let mut y_max = f64::NEG_INFINITY;
        for poly in polys {
            for c in poly.exterior().0.iter() {
                x_min = x_min.min(c.x);
                y_min = y_min.min(c.y);
                x_max = x_max.max(c.x);
                y_max = y_max.max(c.y);
            }
        }
        Some(Rect::new(coord!(x: x_min, y: y_min), coord!(x: x_max, y: y_max)))
    }

    /// Merge all polygons for `(layer, datatype)` in-place -- equivalent to
    /// KLayout's `Region.merged()`.  The existing entry is replaced with the
    /// non-overlapping result.  No-op if the layer is absent.
    pub fn merge(&mut self, layer: i16, datatype: i16) {
        if let Some(polys) = self.inner.get_mut(&(layer, datatype)) {
            *polys = merge_polygons(polys);
        }
    }

    /// Compute the inner boundary of a ring-shaped layer (e.g. SealRing/EdgeSeal).
    ///
    /// Each polygon is assigned to a quadrant by its centroid relative to the
    /// chip centre.  The innermost edge of each quadrant's polygons defines one
    /// side of the returned rectangle:
    ///
    /// - left  quadrant  -> max(bbox.max_x)  -> inner left  edge
    /// - right quadrant  -> min(bbox.min_x)  -> inner right edge
    /// - bottom quadrant -> max(bbox.max_y)  -> inner bottom edge
    /// - top   quadrant  -> min(bbox.min_y)  -> inner top   edge
    ///
    /// Corner/staircase polygons (centroid in a diagonal quadrant) naturally
    /// constrain both the horizontal and vertical inner boundaries.
    /// Polygons whose centroid lies exactly on a centre axis are skipped for
    /// that axis so that full-width/height strips don't falsely constrain the
    /// perpendicular dimension.
    pub fn inner_rect(&self, layer: i16, datatype: i16, chip_bbox: Rect<f64>) -> Option<Rect<f64>> {
        let polys = self.polygons(layer, datatype);
        if polys.is_empty() {
            return None;
        }

        let cx = (chip_bbox.min().x + chip_bbox.max().x) / 2.0;
        let cy = (chip_bbox.min().y + chip_bbox.max().y) / 2.0;

        let mut x_min = chip_bbox.min().x;
        let mut y_min = chip_bbox.min().y;
        let mut x_max = chip_bbox.max().x;
        let mut y_max = chip_bbox.max().y;

        for poly in polys {
            let Some(bb) = poly.bounding_rect() else { continue };
            let pcx = (bb.min().x + bb.max().x) / 2.0;
            let pcy = (bb.min().y + bb.max().y) / 2.0;

            if pcx < cx { x_min = x_min.max(bb.max().x); }
            else if pcx > cx { x_max = x_max.min(bb.min().x); }

            if pcy < cy { y_min = y_min.max(bb.max().y); }
            else if pcy > cy { y_max = y_max.min(bb.min().y); }
        }

        (x_min < x_max && y_min < y_max)
            .then(|| Rect::new(coord!(x: x_min, y: y_min), coord!(x: x_max, y: y_max)))
    }

    /// Remove polygons whose bounding box is fully contained within another
    /// polygon's bounding box on `(layer, datatype)`.  No-op if layer is absent.
    pub fn remove_contained(&mut self, layer: i16, datatype: i16) {
        if let Some(polys) = self.inner.get_mut(&(layer, datatype)) {
            *polys = remove_contained_polygons(polys);
        }
    }


    /// Free the polygon data for `(layer, datatype)`, releasing its memory.
    pub fn drop(&mut self, layer: i16, datatype: i16) {
        self.inner.remove(&(layer, datatype));
    }
}

/// Union all polygons in `polys` into a non-overlapping set.
/// Overlapping shapes (e.g. stacked vias, IO filler cells) are merged so that
/// density calculations do not double-count shared area.
///
/// Uses i_overlay's sweep-line engine via `simplify_shape` -- a single O(n log n)
/// pass over all edges, equivalent to KLayout's `Region.merged()`.
pub fn merge_polygons(polys: &[Polygon<f64>]) -> Vec<Polygon<f64>> {
    if polys.len() <= 1 {
        return polys.to_vec();
    }

    // Convert geo polygons -> i_overlay shape format: Vec<Vec<Vec<[f64; 2]>>>
    // Each shape is a list of contours; geo rings close with a repeated point -- drop it.
    let shapes: Vec<Vec<Vec<[f64; 2]>>> = polys.iter()
        .map(|p| {
            // i_overlay expects CCW exterior contours for NonZero fill.
            // GDS stores polygons in arbitrary winding order, so normalise:
            // positive signed area -> CCW (correct), negative -> CW -> reverse.
            let ext = p.exterior();
            let mut pts: Vec<[f64; 2]> = ext.0.iter()
                .take(ext.0.len().saturating_sub(1))
                .map(|c| [c.x, c.y])
                .collect();
            if p.signed_area() < 0.0 {
                pts.reverse();
            }
            let mut contours: Vec<Vec<[f64; 2]>> = vec![pts];
            for hole in p.interiors() {
                contours.push(
                    hole.0.iter()
                        .take(hole.0.len().saturating_sub(1))
                        .map(|c| [c.x, c.y])
                        .collect()
                );
            }
            contours
        })
        .collect();

    // Single sweep-line pass over all edges.
    let merged = shapes.simplify_shape(FillRule::NonZero);

    // Convert back: i_overlay contours are open (no repeated closing point).
    merged.into_iter()
        .filter_map(|shape| {
            let mut contours = shape.into_iter();
            let outer = contours.next()?;
            let mut exterior: Vec<geo::Coord<f64>> =
                outer.iter().map(|p| coord!(x: p[0], y: p[1])).collect();
            if let Some(&first) = exterior.first() {
                exterior.push(first);
            }
            let holes: Vec<LineString<f64>> = contours
                .map(|h| {
                    let mut pts: Vec<geo::Coord<f64>> =
                        h.iter().map(|p| coord!(x: p[0], y: p[1])).collect();
                    if let Some(&first) = pts.first() {
                        pts.push(first);
                    }
                    LineString::new(pts)
                })
                .collect();
            Some(Polygon::new(LineString::new(exterior), holes))
        })
        .collect()
}

/// Remove polygons whose bounding box is fully contained within another polygon's
/// bounding box. Eliminates redundant nested shapes (e.g. cell metal inside a
/// power stripe) before density calculations to avoid double-counting.
///
/// Uses bounding boxes -- exact for rectangular shapes, approximate otherwise.
/// Runs in O(n * k) where k is the number of distinct containers found; for
/// typical layouts with few large container shapes this is effectively O(n).
/// Returns true if `poly` is an axis-aligned rectangle
/// (exactly 4 unique corners, area == bbox area).
fn is_rect(poly: &Polygon<f64>) -> bool {
    if poly.exterior().0.len() != 5 { return false; }
    let Some(bbox) = poly.bounding_rect() else { return false };
    let bbox_area = (bbox.max().x - bbox.min().x) * (bbox.max().y - bbox.min().y);
    if bbox_area < 1e-12 { return false; }
    (bbox_area - poly.unsigned_area()).abs() / bbox_area < 1e-6
}

pub fn remove_contained_polygons(polys: &[Polygon<f64>]) -> Vec<Polygon<f64>> {
    if polys.len() <= 1 {
        return polys.to_vec();
    }

    // Step 1 (parallel): compute bboxes for rectangular polygons only.
    // Non-rectangular shapes (L, T, ring, ...) are always kept.
    let mut bboxes: Vec<(usize, Rect<f64>)> = polys.par_iter()
        .enumerate()
        .filter_map(|(i, p)| {
            if is_rect(p) { p.bounding_rect().map(|b| (i, b)) } else { None }
        })
        .collect();

    // Step 2: sort largest area first.
    let rect_area = |r: &Rect<f64>| (r.max().x - r.min().x) * (r.max().y - r.min().y);
    bboxes.sort_by(|a, b| {
        rect_area(&b.1).partial_cmp(&rect_area(&a.1)).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Step 3 (sequential, bounded): scan the top MAX_SCAN largest bboxes to
    // build the containers list.  Containers are always among the largest shapes,
    // so capping the scan avoids the O(n²) worst case that occurs when every
    // polygon has a unique size (e.g. Activ standard-cell transistors).
    const MAX_SCAN: usize = 8192;
    let mut containers: Vec<Rect<f64>> = Vec::new();
    for (_, bbox) in bboxes.iter().take(MAX_SCAN) {
        let is_sub = containers.iter().any(|c| {
            c.min().x <= bbox.min().x && bbox.max().x <= c.max().x
                && c.min().y <= bbox.min().y && bbox.max().y <= c.max().y
        });
        if !is_sub {
            containers.push(*bbox);
        }
    }

    // Step 4 (parallel): mark each rect bbox as contained if a strictly-larger
    // container encloses it.  The area guard prevents a container from matching
    // itself (equal-area case after dedup).
    let mut contained = vec![false; polys.len()];
    let flags: Vec<(usize, bool)> = bboxes.par_iter()
        .map(|(idx, bbox)| {
            let flag = containers.iter().any(|c| {
                rect_area(c) > rect_area(bbox) + 1e-6
                    && c.min().x <= bbox.min().x && bbox.max().x <= c.max().x
                    && c.min().y <= bbox.min().y && bbox.max().y <= c.max().y
            });
            (*idx, flag)
        })
        .collect();
    for (idx, flag) in flags {
        contained[idx] = flag;
    }

    // Step 5 (parallel): collect non-contained polygons.
    polys.par_iter().enumerate()
        .filter_map(|(i, p)| if contained[i] { None } else { Some(p.clone()) })
        .collect()
}


/// Convert a GDS path element into one rectangular polygon per segment.
///
/// Each segment is expanded by `width/2` perpendicular to its direction.
/// End-cap extensions follow `path_type` (0/1 = flush, 2 = half-width, 4 = explicit).
/// At multi-segment joins the rectangles overlap slightly; callers that need exact
/// area (density calculation) must merge the result with `tiled_merge_area`.
fn path_to_polygons(path: &GdsPath, xform: &AffineTransform) -> Vec<Polygon<f64>> {
    let width = match path.width { Some(w) if w > 0 => w as f64, _ => return vec![] };
    let hw = width / 2.0;

    let pts: Vec<(f64, f64)> = path.xy.iter()
        .map(|pt| xform.apply(pt.x as f64, pt.y as f64))
        .collect();
    if pts.len() < 2 { return vec![]; }

    let n = pts.len();
    let mut out = Vec::with_capacity(n - 1);

    for (i, w) in pts.windows(2).enumerate() {
        let (x1, y1) = w[0];
        let (x2, y2) = w[1];

        let dx = x2 - x1;
        let dy = y2 - y1;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-9 { continue; }

        let ux = dx / len;
        let uy = dy / len;
        // Left-perpendicular (CCW)
        let px = -uy * hw;
        let py =  ux * hw;

        // End-cap extensions: only apply at the actual path ends, not at joins.
        let (ext_begin, ext_end) = match path.path_type.unwrap_or(0) {
            2 => (
                if i == 0     { hw } else { 0.0 },
                if i == n - 2 { hw } else { 0.0 },
            ),
            4 => (
                if i == 0     { path.begin_extn.unwrap_or(0) as f64 } else { 0.0 },
                if i == n - 2 { path.end_extn.unwrap_or(0)   as f64 } else { 0.0 },
            ),
            _ => (0.0, 0.0),
        };

        let ax = x1 - ux * ext_begin - px;  let ay = y1 - uy * ext_begin - py;
        let bx = x2 + ux * ext_end   - px;  let by = y2 + uy * ext_end   - py;
        let cx = x2 + ux * ext_end   + px;  let cy = y2 + uy * ext_end   + py;
        let ex = x1 - ux * ext_begin + px;  let ey = y1 - uy * ext_begin + py;

        let coords = vec![
            coord!(x: ax, y: ay),
            coord!(x: bx, y: by),
            coord!(x: cx, y: cy),
            coord!(x: ex, y: ey),
            coord!(x: ax, y: ay),
        ];
        out.push(Polygon::new(LineString::new(coords), vec![]));
    }
    out
}

fn collect_all_recursive<'a>(
    s: &'a GdsStruct,
    struct_map: &HashMap<&str, &'a GdsStruct>,
    needed: Option<&HashSet<(i16, i16)>>,
    xform: &AffineTransform,
    map: &mut HashMap<(i16, i16), Vec<Polygon<f64>>>,
) {
    for elem in &s.elems {
        match elem {
            GdsElement::GdsBoundary(b) => {
                if needed.is_some_and(|n| !n.contains(&(b.layer, b.datatype))) {
                    continue;
                }
                let coords = b.xy.iter()
                    .map(|p| { let (x, y) = xform.apply(p.x as f64, p.y as f64); coord!(x: x, y: y) })
                    .collect::<Vec<_>>();
                map.entry((b.layer, b.datatype))
                    .or_default()
                    .push(Polygon::new(LineString::new(coords), vec![]));
            }
            GdsElement::GdsPath(p) => {
                if needed.is_some_and(|n| !n.contains(&(p.layer, p.datatype))) {
                    continue;
                }
                for poly in path_to_polygons(p, xform) {
                    map.entry((p.layer, p.datatype)).or_default().push(poly);
                }
            }
            GdsElement::GdsStructRef(r) => {
                if let Some(child) = struct_map.get(r.name.as_str()) {
                    let child_xform = xform.compose(&AffineTransform::from_sref(r));
                    collect_all_recursive(child, struct_map, needed, &child_xform, map);
                }
            }
            GdsElement::GdsArrayRef(a) => {
                if let Some(child) = struct_map.get(a.name.as_str()) {
                    for inst_xform in aref_instance_transforms(a, xform) {
                        collect_all_recursive(child, struct_map, needed, &inst_xform, map);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Expand an `GdsArrayRef` into per-instance `AffineTransform`s.
///
/// The three `xy` points encode the array layout in the parent coordinate system:
/// - `xy[0]`: array origin
/// - `xy[1]`: position of instance `(cols, 0)` -> column pitch
/// - `xy[2]`: position of instance `(0, rows)` -> row pitch
fn aref_instance_transforms(a: &GdsArrayRef, parent: &AffineTransform) -> Vec<AffineTransform> {
    let cols = a.cols as f64;
    let rows = a.rows as f64;
    let ox = a.xy[0].x as f64;
    let oy = a.xy[0].y as f64;
    let col_dx = (a.xy[1].x as f64 - ox) / cols;
    let col_dy = (a.xy[1].y as f64 - oy) / cols;
    let row_dx = (a.xy[2].x as f64 - ox) / rows;
    let row_dy = (a.xy[2].y as f64 - oy) / rows;

    let mut out = Vec::with_capacity(a.cols as usize * a.rows as usize);
    for col in 0..a.cols {
        for row in 0..a.rows {
            let tx = ox + col as f64 * col_dx + row as f64 * row_dx;
            let ty = oy + col as f64 * col_dy + row as f64 * row_dy;
            let inst = AffineTransform::from_strans_xy(a.strans.as_ref(), tx, ty);
            out.push(parent.compose(&inst));
        }
    }
    out
}


// Geometry helpers

/// Offset a polygon outward by `amount` using vertex-normal bisectors.
pub fn offset_polygon(poly: &Polygon<f64>, amount: f64) -> Polygon<f64> {
    let pts = &poly.exterior().0;
    let n = pts.len();
    if n < 4 {
        return poly.clone();
    }
    let m = n - 1;

    let outward: f64 = if poly.signed_area() >= 0.0 { 1.0 } else { -1.0 };

    let mut new_pts: Vec<geo::Coord<f64>> = Vec::with_capacity(n);
    for i in 0..m {
        let prev = pts[(i + m - 1) % m];
        let curr = pts[i];
        let next = pts[(i + 1) % m];

        let e1x = curr.x - prev.x; let e1y = curr.y - prev.y;
        let e2x = next.x - curr.x; let e2y = next.y - curr.y;
        let l1 = e1x.hypot(e1y);
        let l2 = e2x.hypot(e2y);

        if l1 < 1e-10 || l2 < 1e-10 {
            new_pts.push(curr);
            continue;
        }

        let n1x = outward * e1y / l1;  let n1y = -outward * e1x / l1;
        let n2x = outward * e2y / l2;  let n2y = -outward * e2x / l2;

        let bx = n1x + n2x;
        let by = n1y + n2y;
        let blen = bx.hypot(by);

        let (dx, dy) = if blen < 1e-10 {
            (n1x * amount, n1y * amount)
        } else {
            let scale = (amount / (blen / 2.0)).min(amount * 4.0);
            (bx / blen * scale, by / blen * scale)
        };

        new_pts.push(coord!(x: curr.x + dx, y: curr.y + dy));
    }
    new_pts.push(new_pts[0]);
    Polygon::new(LineString::new(new_pts), vec![])
}

/// Offset each polygon outward by `amount` in parallel. Zero-amount returns clones.
pub fn offset_polygons(polys: &[Polygon<f64>], amount: f64) -> Vec<Polygon<f64>> {
    if amount == 0.0 {
        return polys.to_vec();
    }
    polys.par_iter().map(|p| offset_polygon(p, amount)).collect()
}

// Tile geometry / density helpers

/// Compute the covered X-length of a set of intervals sorted by their left
/// endpoint.  Overlapping intervals are merged on the fly in O(n).
pub fn covered_x_length(sorted: &[(f64, f64)]) -> f64 {
    let mut total = 0.0f64;
    let mut hi = f64::NEG_INFINITY;
    for &(x0, x1) in sorted {
        if x0 > hi      { total += x1 - x0; hi = x1; }
        else if x1 > hi { total += x1 - hi; hi = x1; }
    }
    total
}

/// Union area of a set of axis-aligned rectangles using Klee's scanline algorithm.
/// Memory: O(n) for the event list and active-interval set.  Time: O(n log n).
pub fn rect_union_area(rects: &[(f64, f64, f64, f64)]) -> f64 {
    if rects.is_empty() { return 0.0; }

    // Events: (y, is_enter, x0, x1)
    let mut events: Vec<(f64, bool, f64, f64)> = rects.iter()
        .flat_map(|&(x0, y0, x1, y1)| [(y0, true, x0, x1), (y1, false, x0, x1)])
        .collect();
    events.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    let mut active: Vec<(f64, f64)> = Vec::new(); // sorted by x0
    let mut y_prev = f64::NEG_INFINITY;
    let mut area   = 0.0f64;
    let mut i      = 0;

    while i < events.len() {
        let y = events[i].0;
        // Accumulate area for the strip [y_prev, y] before updating active set.
        if y > y_prev && !active.is_empty() {
            area += (y - y_prev) * covered_x_length(&active);
        }
        // Process all events at the same y.
        while i < events.len() && events[i].0 == y {
            let (_, enter, x0, x1) = events[i];
            if enter {
                let pos = active.partition_point(|&(ax, _)| ax < x0);
                active.insert(pos, (x0, x1));
            } else if let Some(p) = active.iter().position(|&(ax0, ax1)|
                (ax0 - x0).abs() < 1e-10 && (ax1 - x1).abs() < 1e-10)
            {
                active.remove(p);
            }
            i += 1;
        }
        y_prev = y;
    }
    area
}

/// Area of the union of `polys` without constructing merged geometry.
///
/// Axis-aligned rectangles (the vast majority of GDS shapes) are handled by
/// Klee's scanline algorithm in O(n log n) time and O(n) memory.
/// Non-rectangular polygons are assumed DRC-clean (no mutual overlaps) and
/// their areas are summed directly -- a safe approximation for standard layouts.
pub fn union_area(polys: &[Polygon<f64>]) -> f64 {
    if polys.is_empty() { return 0.0; }
    if polys.len()  == 1 { return polys[0].unsigned_area(); }

    let mut non_rect_area = 0.0f64;
    let mut rects: Vec<(f64, f64, f64, f64)> = Vec::new();

    for p in polys {
        let Some(b) = p.bounding_rect() else { continue };
        let bbox_area = (b.max().x - b.min().x) * (b.max().y - b.min().y);
        let is_rect   = p.exterior().0.len() == 5
            && bbox_area > 1e-12
            && (bbox_area - p.unsigned_area()).abs() / bbox_area < 1e-6;
        if is_rect {
            rects.push((b.min().x, b.min().y, b.max().x, b.max().y));
        } else {
            non_rect_area += p.unsigned_area();
        }
    }

    non_rect_area + rect_union_area(&rects)
}

/// Sum of areas clipped to `tile` for the polygons at `indices`.
///
/// Polygons must be non-overlapping before this is called; otherwise
/// shared area is double-counted.
pub fn clipped_area(polys: &[Polygon<f64>], indices: &[usize], tile: &Polygon<f64>) -> f64 {
    indices.iter()
        .map(|&ki| polys[ki].intersection(tile).unsigned_area())
        .sum::<f64>()
        .max(0.0)
}

/// Compute the merged area of the polygons at `candidates` within `tile_rect`,
/// using sub-windows of `window_dbu` to bound peak memory.  Each sub-window
/// clips its candidates, merges them, and sums their area -- correct because
/// area is additive over non-overlapping tiles.  When `window_dbu` is `None`
/// the whole tile is treated as one window.
///
/// Takes indices into `polys` instead of a pre-cloned slice to avoid a
/// potentially large intermediate allocation.
pub fn tiled_merge_area(
    polys: &[Polygon<f64>],
    candidates: &[usize],
    tile_rect: Rect<f64>,
    window_dbu: Option<f64>,
) -> f64 {
    let tx0 = tile_rect.min().x;
    let ty0 = tile_rect.min().y;
    let tx1 = tile_rect.max().x;
    let ty1 = tile_rect.max().y;

    let window = window_dbu.unwrap_or((tx1 - tx0).max(ty1 - ty0));
    let nx = ((tx1 - tx0) / window).ceil() as usize;
    let ny = ((ty1 - ty0) / window).ceil() as usize;

    let mut total = 0.0f64;
    for iy in 0..ny {
        for ix in 0..nx {
            let wx0 = tx0 + ix as f64 * window;
            let wx1 = (wx0 + window).min(tx1);
            let wy0 = ty0 + iy as f64 * window;
            let wy1 = (wy0 + window).min(ty1);
            let win_poly = Rect::new(coord!(x: wx0, y: wy0), coord!(x: wx1, y: wy1)).to_polygon();

            let clipped: Vec<Polygon<f64>> = candidates.iter()
                .filter_map(|&ki| {
                    let p = &polys[ki];
                    let bbox = p.bounding_rect()?;
                    if bbox.min().x >= wx1 || bbox.max().x <= wx0
                        || bbox.min().y >= wy1 || bbox.max().y <= wy0
                    {
                        return None;
                    }
                    // Fully contained -- no geometric clip needed.
                    if bbox.min().x >= wx0 && bbox.max().x <= wx1
                        && bbox.min().y >= wy0 && bbox.max().y <= wy1
                    {
                        return Some(vec![p.clone()]);
                    }
                    Some(p.intersection(&win_poly).0)
                })
                .flatten()
                .collect();

            if clipped.is_empty() { continue; }

            // union_area uses Klee's scanline -- no i_overlay, O(n) peak memory.
            total += union_area(&clipped);
        }
    }
    total
}

/// Build a flat spatial index over `polys` for a regular tile grid.
///
/// Returns a `Vec` of length `nx * ny`; entry `iy * nx + ix` holds the
/// indices of every polygon whose bounding box overlaps that tile cell.
pub fn build_tile_index(
    polys: &[Polygon<f64>],
    x_min: f64, y_min: f64,
    tile_size: f64,
    nx: usize, ny: usize,
) -> Vec<Vec<usize>> {
    let mut idx = vec![vec![]; nx * ny];
    for (ki, poly) in polys.iter().enumerate() {
        let Some(bbox) = poly.bounding_rect() else { continue };
        let ix0 = (((bbox.min().x - x_min) / tile_size).floor() as isize).clamp(0, nx as isize - 1) as usize;
        let iy0 = (((bbox.min().y - y_min) / tile_size).floor() as isize).clamp(0, ny as isize - 1) as usize;
        let ix1 = (((bbox.max().x - x_min) / tile_size).floor() as isize).clamp(0, nx as isize - 1) as usize;
        let iy1 = (((bbox.max().y - y_min) / tile_size).floor() as isize).clamp(0, ny as isize - 1) as usize;
        for iy in iy0..=iy1 {
            for ix in ix0..=ix1 {
                idx[iy * nx + ix].push(ki);
            }
        }
    }
    idx
}
