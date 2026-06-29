use glam::Vec3;
use serde::{Deserialize, Serialize};

const MAX_LEAF_COUNT: usize = 8;

#[repr(C)]
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Aabb {
    min: Vec3,
    max: Vec3,
}

impl Aabb {
    pub fn new() -> Self {
        Self {
            min: Vec3::splat(f32::MAX),
            max: Vec3::splat(f32::MIN),
        }
    }

    #[allow(unused)]
    pub fn min(&self) -> &Vec3 {
        &self.min
    }

    #[allow(unused)]
    pub fn max(&self) -> &Vec3 {
        &self.max
    }

    pub fn centroid(&self) -> Vec3 {
        (self.min + self.max) / 2.0
    }

    pub fn from_points(points: &[Vec3]) -> Self {
        let mut aabb = Aabb::new();
        for &p in points {
            aabb.expand(p);
        }
        aabb
    }

    pub fn expand(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn merge(&self, other: &Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    pub fn ray_intersect(&self, origin: Vec3, inv_dir: Vec3, max_t: f32) -> bool {
        let t1 = (self.min - origin) * inv_dir;
        let t2 = (self.max - origin) * inv_dir;

        let tmin = t1.min(t2);
        let tmax = t1.max(t2);

        let t_min = tmin.x.max(tmin.y).max(tmin.z);
        let t_max = tmax.x.min(tmax.y).min(tmax.z);

        t_min <= t_max && t_min <= max_t && t_max >= 0.0
    }
}

#[repr(C)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Triangle {
    pub v0: Vec3,
    pub v1: Vec3,
    pub v2: Vec3,
}

/// A single solid span the ray passed through, used by penetration calculations.
#[derive(Debug, Clone, Copy)]
pub struct WallHit {
    /// Surface material index (see `cs2::penetration` constants).
    /// Currently always `SURFACE_DEFAULT` until material parsing lands.
    pub surface_index: u8,
    /// World-unit thickness of the solid along the ray.
    pub thickness: f32,
}

impl Triangle {
    pub fn aabb(&self) -> Aabb {
        Aabb::from_points(&[self.v0, self.v1, self.v2])
    }

    pub fn centroid(&self) -> Vec3 {
        (self.v0 + self.v1 + self.v2) * (1.0 / 3.0)
    }

    /// Geometric (un-normalized) face normal.
    pub fn normal(&self) -> Vec3 {
        (self.v1 - self.v0).cross(self.v2 - self.v0)
    }

    pub fn ray_intersect(&self, origin: Vec3, dir: Vec3) -> Option<(f32, f32, f32)> {
        const EPSILON: f32 = 1e-6;
        let edge1 = self.v1 - self.v0;
        let edge2 = self.v2 - self.v0;
        let h = dir.cross(edge2);
        let a = edge1.dot(h);

        if a > -EPSILON && a < EPSILON {
            return None;
        }

        let f = 1.0 / a;
        let s = origin - self.v0;
        let u = f * s.dot(h);

        if !(0.0..=1.0).contains(&u) {
            return None;
        }

        let q = s.cross(edge1);
        let v = f * dir.dot(q);

        if v < 0.0 || u + v > 1.0 {
            return None;
        }

        let t = f * edge2.dot(q);
        if t > EPSILON { Some((t, u, v)) } else { None }
    }
}

#[repr(C)]
#[derive(Debug, Serialize, Deserialize)]
enum BvhNode {
    Branch {
        left: usize,
        right: usize,
        aabb: Aabb,
    },
    Leaf {
        primitives: Vec<usize>,
        aabb: Aabb,
    },
}

#[repr(C)]
#[derive(Debug, Serialize, Deserialize)]
pub struct Bvh {
    nodes: Vec<BvhNode>,
    triangles: Vec<Triangle>,
    root: Option<usize>,
}

impl Bvh {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            triangles: Vec::new(),
            root: None,
        }
    }

    pub fn set(&mut self, triangles: Vec<Triangle>) {
        self.triangles = triangles;
    }

    /// Serialize a fully-built BVH to disk (bincode). Used by the on-disk
    /// cache so a map's tree is built once and reloaded instantly thereafter.
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = bincode::serialize(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        // Write to a temp file then rename, so a crash mid-write can't leave a
        // half-written cache file that would later deserialize into garbage.
        let tmp = path.with_extension("bvh.tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Load a previously-built BVH from disk. Returns `None` on any error
    /// (missing, corrupt, or version-mismatched file) so the caller can fall
    /// back to rebuilding from live geometry.
    pub fn load(path: &std::path::Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        bincode::deserialize(&bytes).ok()
    }

    #[allow(unused)]
    pub fn triangles(&self, position: &Vec3) -> Vec<&Triangle> {
        self.triangles
            .iter()
            .filter(|tri| (tri.centroid() - position).length() < 1000.0)
            .collect()
    }

    #[allow(unused)]
    pub fn aabbs(&self, position: &Vec3) -> Vec<&Aabb> {
        self.nodes
            .iter()
            .filter_map(|node| {
                let aabb = node.aabb();
                if (aabb.centroid() - position).length() < 1000.0 {
                    Some(aabb)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn build(&mut self) {
        if self.triangles.is_empty() {
            self.root = None;
            return;
        }

        // Precompute every triangle centroid once. The previous code recomputed
        // centroids inside the sort comparator at every tree level, which for
        // millions of triangles meant billions of redundant centroid
        // calculations (tens of seconds in debug builds).
        let centroids: Vec<Vec3> = self.triangles.iter().map(|t| t.centroid()).collect();

        let mut primitives: Vec<usize> = (0..self.triangles.len()).collect();
        self.nodes.clear();
        self.nodes.reserve(self.triangles.len() / 4);
        self.root = Some(self.build_recursive(&mut primitives, &centroids));
    }

    fn build_recursive(&mut self, primitives: &mut [usize], centroids: &[Vec3]) -> usize {
        if primitives.len() <= MAX_LEAF_COUNT {
            let aabb = primitives.iter().fold(Aabb::new(), |acc, &idx| {
                acc.merge(&self.triangles[idx].aabb())
            });
            return self.create_leaf(primitives, aabb);
        }

        let centroid_bounds = primitives.iter().fold(Aabb::new(), |mut acc, &idx| {
            acc.expand(centroids[idx]);
            acc
        });

        let extent = centroid_bounds.max - centroid_bounds.min;
        let axis = if extent.x > extent.y && extent.x > extent.z {
            0
        } else if extent.y > extent.z {
            1
        } else {
            2
        };

        primitives.sort_unstable_by(|&a_idx, &b_idx| {
            centroids[a_idx][axis]
                .partial_cmp(&centroids[b_idx][axis])
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mid = primitives.len() / 2;
        let (left_prims, right_prims) = primitives.split_at_mut(mid);

        let left = self.build_recursive(left_prims, centroids);
        let right = self.build_recursive(right_prims, centroids);

        let left_aabb = self.nodes[left].aabb();
        let right_aabb = self.nodes[right].aabb();
        let aabb = left_aabb.merge(right_aabb);

        self.create_internal(left, right, aabb)
    }

    fn create_leaf(&mut self, primitives: &[usize], aabb: Aabb) -> usize {
        let node = BvhNode::Leaf {
            primitives: primitives.to_vec(),
            aabb,
        };
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    fn create_internal(&mut self, left: usize, right: usize, aabb: Aabb) -> usize {
        let node = BvhNode::Branch { left, right, aabb };
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    pub fn has_line_of_sight(&self, start: Vec3, end: Vec3) -> bool {
        let dir = end - start;
        let distance = dir.length();

        let dir_norm = dir / distance;
        let inv_dir = 1.0 / dir_norm;

        if let Some(root) = self.root {
            !self.segment_intersect_node(root, start, dir_norm, inv_dir, distance)
        } else {
            true
        }
    }

    fn segment_intersect_node(
        &self,
        node_idx: usize,
        origin: Vec3,
        direction: Vec3,
        inv_dir: Vec3,
        max_t: f32,
    ) -> bool {
        let node = &self.nodes[node_idx];

        if !node.aabb().ray_intersect(origin, inv_dir, max_t) {
            return false;
        }

        match node {
            BvhNode::Leaf { primitives, .. } => {
                for &idx in primitives {
                    if let Some((t, _, _)) = self.triangles[idx].ray_intersect(origin, direction)
                        && t >= 0.0
                        && t <= max_t
                    {
                        return true;
                    }
                }
                false
            }
            BvhNode::Branch { left, right, .. } => {
                self.segment_intersect_node(*left, origin, direction, inv_dir, max_t)
                    || self.segment_intersect_node(*right, origin, direction, inv_dir, max_t)
            }
        }
    }

    /// Cast a ray from `start` to `end` and return every solid span (wall) the
    /// ray passes through, with its thickness in world units.
    ///
    /// Triangles are single-sided faces, so we collect all intersections along
    /// the ray and pair them up: a face whose normal opposes the ray direction
    /// is an *entry* into solid, one that agrees is an *exit*. A depth counter
    /// over the sorted hits turns those into enter/exit pairs, which is robust
    /// to nested or overlapping geometry. Thickness = exit_t - enter_t.
    pub fn cast_walls(&self, start: Vec3, end: Vec3) -> Vec<WallHit> {
        let dir = end - start;
        let distance = dir.length();
        if distance <= f32::EPSILON {
            return Vec::new();
        }
        let dir_norm = dir / distance;
        let inv_dir = 1.0 / dir_norm;

        let Some(root) = self.root else {
            return Vec::new();
        };

        // (t along ray, is_entry) for every face hit within the segment.
        let mut hits: Vec<(f32, bool)> = Vec::new();
        self.collect_face_hits(root, start, dir_norm, inv_dir, distance, &mut hits);

        if hits.is_empty() {
            return Vec::new();
        }

        hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut walls = Vec::new();
        let mut depth: i32 = 0;
        let mut enter_t = 0.0_f32;

        for (t, is_entry) in hits {
            if is_entry {
                if depth == 0 {
                    enter_t = t;
                }
                depth += 1;
            } else if depth > 0 {
                depth -= 1;
                if depth == 0 {
                    let thickness = t - enter_t;
                    if thickness > 0.0 {
                        walls.push(WallHit {
                            surface_index: crate::cs2::penetration::SURFACE_DEFAULT,
                            thickness,
                        });
                    }
                }
            }
        }

        walls
    }

    fn collect_face_hits(
        &self,
        node_idx: usize,
        origin: Vec3,
        direction: Vec3,
        inv_dir: Vec3,
        max_t: f32,
        hits: &mut Vec<(f32, bool)>,
    ) {
        let node = &self.nodes[node_idx];

        if !node.aabb().ray_intersect(origin, inv_dir, max_t) {
            return;
        }

        match node {
            BvhNode::Leaf { primitives, .. } => {
                for &idx in primitives {
                    let tri = &self.triangles[idx];
                    if let Some((t, _, _)) = tri.ray_intersect(origin, direction)
                        && t >= 0.0
                        && t <= max_t
                    {
                        // Entry face: normal points back against the ray.
                        let is_entry = tri.normal().dot(direction) < 0.0;
                        hits.push((t, is_entry));
                    }
                }
            }
            BvhNode::Branch { left, right, .. } => {
                self.collect_face_hits(*left, origin, direction, inv_dir, max_t, hits);
                self.collect_face_hits(*right, origin, direction, inv_dir, max_t, hits);
            }
        }
    }

    #[allow(unused)]
    pub fn triangles_near(&self, position: Vec3, radius: f32) -> Vec<&Triangle> {
        let mut result = Vec::new();
        if let Some(root) = self.root {
            self.collect_triangles_near(root, position, radius, &mut result);
        }
        result
    }

    #[allow(unused)]
    pub fn aabbs_near(&self, position: Vec3, radius: f32) -> Vec<&Aabb> {
        let mut result = Vec::new();
        if let Some(root) = self.root {
            self.collect_aabbs_near(root, position, radius, &mut result);
        }
        result
    }

    #[allow(unused)]
    fn collect_triangles_near<'a>(
        &'a self,
        node_idx: usize,
        position: Vec3,
        radius: f32,
        result: &mut Vec<&'a Triangle>,
    ) {
        let node = &self.nodes[node_idx];
        let aabb = node.aabb();

        if !self.sphere_aabb_intersect(position, radius, aabb) {
            return;
        }

        match node {
            BvhNode::Leaf { primitives, .. } => {
                for &idx in primitives {
                    let tri = &self.triangles[idx];
                    if (tri.centroid() - position).length() <= radius {
                        result.push(tri);
                    }
                }
            }
            BvhNode::Branch { left, right, .. } => {
                self.collect_triangles_near(*left, position, radius, result);
                self.collect_triangles_near(*right, position, radius, result);
            }
        }
    }

    #[allow(unused)]
    fn collect_aabbs_near<'a>(
        &'a self,
        node_idx: usize,
        position: Vec3,
        radius: f32,
        result: &mut Vec<&'a Aabb>,
    ) {
        let node = &self.nodes[node_idx];
        let aabb = node.aabb();

        if !self.sphere_aabb_intersect(position, radius, aabb) {
            return;
        }

        if (aabb.centroid() - position).length() <= radius {
            result.push(aabb);
        }

        if let BvhNode::Branch { left, right, .. } = node {
            self.collect_aabbs_near(*left, position, radius, result);
            self.collect_aabbs_near(*right, position, radius, result);
        }
    }

    #[allow(unused)]
    fn sphere_aabb_intersect(&self, sphere_center: Vec3, sphere_radius: f32, aabb: &Aabb) -> bool {
        let closest_point = sphere_center.clamp(aabb.min, aabb.max);
        let distance_sq = (sphere_center - closest_point).length_squared();
        distance_sq <= sphere_radius * sphere_radius
    }
}

impl BvhNode {
    fn aabb(&self) -> &Aabb {
        match self {
            BvhNode::Branch { aabb, .. } => aabb,
            BvhNode::Leaf { aabb, .. } => aabb,
        }
    }
}
