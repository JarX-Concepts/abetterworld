use cgmath::{Matrix3, SquareMatrix, Vector3, Zero};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub min: Vector3<f64>,
    pub max: Vector3<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrientedBoundingBox {
    pub center: Vector3<f64>,
    pub half_axes: [Vector3<f64>; 3], // U, V, W
}

pub struct Ray {
    pub origin: Vector3<f64>,
    pub direction: Vector3<f64>, // Assumed normalized
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
pub struct BoundingVolume {
    #[serde(rename = "box")]
    bounding_box: [f64; 12],
}

impl BoundingVolume {
    pub fn to_obb(&self) -> OrientedBoundingBox {
        let b = &self.bounding_box;

        let center = Vector3::new(b[0], b[1], b[2]);
        let half_axes = [
            Vector3::new(b[3], b[4], b[5]),
            Vector3::new(b[6], b[7], b[8]),
            Vector3::new(b[9], b[10], b[11]),
        ];

        OrientedBoundingBox { center, half_axes }
    }

    /// Convert this bounding volume to a conservative AABB
    pub fn to_aabb(&self) -> BoundingBox {
        let b = &self.bounding_box;

        let center = Vector3::new(b[0], b[1], b[2]);
        let u = Vector3::new(b[3], b[4], b[5]);
        let v = Vector3::new(b[6], b[7], b[8]);
        let w = Vector3::new(b[9], b[10], b[11]);

        // AABB extents from absolute values of axes
        let extent = Vector3::new(
            u.x.abs() + v.x.abs() + w.x.abs(),
            u.y.abs() + v.y.abs() + w.y.abs(),
            u.z.abs() + v.z.abs() + w.z.abs(),
        );

        BoundingBox {
            min: center - extent,
            max: center + extent,
        }
    }

    pub fn corners(&self, offset: Vector3<f64>) -> [Vector3<f64>; 8] {
        let obb = self.to_obb();
        let center = obb.center - offset;
        let half_axes = obb.half_axes;

        let mut corners = [Vector3::zero(); 8];
        for i in 0..2 {
            for j in 0..2 {
                for k in 0..2 {
                    corners[i * 4 + j * 2 + k] = center
                        + half_axes[0] * (if i == 0 { -1.0 } else { 1.0 })
                        + half_axes[1] * (if j == 0 { -1.0 } else { 1.0 })
                        + half_axes[2] * (if k == 0 { -1.0 } else { 1.0 });
                }
            }
        }
        corners
    }
}

impl BoundingBox {
    pub fn ray_intersect(&self, ray: &Ray) -> Option<f64> {
        let mut t_min = (self.min.x - ray.origin.x) / ray.direction.x;
        let mut t_max = (self.max.x - ray.origin.x) / ray.direction.x;

        if t_min > t_max {
            std::mem::swap(&mut t_min, &mut t_max);
        }

        let mut ty_min = (self.min.y - ray.origin.y) / ray.direction.y;
        let mut ty_max = (self.max.y - ray.origin.y) / ray.direction.y;

        if ty_min > ty_max {
            std::mem::swap(&mut ty_min, &mut ty_max);
        }

        if (t_min > ty_max) || (ty_min > t_max) {
            return None;
        }

        if ty_min > t_min {
            t_min = ty_min;
        }
        if ty_max < t_max {
            t_max = ty_max;
        }

        let mut tz_min = (self.min.z - ray.origin.z) / ray.direction.z;
        let mut tz_max = (self.max.z - ray.origin.z) / ray.direction.z;

        if tz_min > tz_max {
            std::mem::swap(&mut tz_min, &mut tz_max);
        }

        if (t_min > tz_max) || (tz_min > t_max) {
            return None;
        }

        if tz_min > t_min {
            t_min = tz_min;
        }
        if tz_max < t_max {
            t_max = tz_max;
        }

        if t_max < 0.0 {
            return None;
        }

        Some(if t_min >= 0.0 { t_min } else { t_max })
    }
}

impl OrientedBoundingBox {
    pub fn closest_point(&self, point: Vector3<f64>) -> Vector3<f64> {
        let basis = Matrix3::from_cols(self.half_axes[0], self.half_axes[1], self.half_axes[2]);

        let Some(inv_basis) = basis.invert() else {
            log::warn!("OBB basis matrix is not invertible");
            return self.center;
        };

        let local = inv_basis * (point - self.center);

        // Point is inside the box if all local coords are within [-1, 1]
        if local.x.abs() <= 1.0 && local.y.abs() <= 1.0 && local.z.abs() <= 1.0 {
            return point;
        }

        let clamped = Vector3::new(
            local.x.clamp(-1.0, 1.0),
            local.y.clamp(-1.0, 1.0),
            local.z.clamp(-1.0, 1.0),
        );

        self.center + basis * clamped
    }

    pub fn ray_intersect(&self, ray: &Ray) -> Option<f64> {
        let basis = Matrix3::from_cols(self.half_axes[0], self.half_axes[1], self.half_axes[2]);

        log::debug!("OBB center: {:?}", self.center);
        log::debug!("OBB half-axes: {:?}", self.half_axes);
        log::debug!("Ray origin: {:?}", ray.origin);
        log::debug!("Ray direction: {:?}", ray.direction);

        let Some(inv_basis) = basis.invert() else {
            log::warn!("OBB basis matrix is not invertible");
            return None;
        };

        log::debug!("Inverse basis matrix: {:?}", inv_basis);

        let local_origin = inv_basis * (ray.origin - self.center);
        let local_direction = inv_basis * ray.direction;

        log::debug!("Local ray origin: {:?}", local_origin);
        log::debug!("Local ray direction: {:?}", local_direction);

        let min = Vector3::new(-1.0, -1.0, -1.0);
        let max = Vector3::new(1.0, 1.0, 1.0);

        let mut t_min = f64::NEG_INFINITY;
        let mut t_max = f64::INFINITY;

        for i in 0..3 {
            let origin = local_origin[i];
            let dir = local_direction[i];

            log::debug!(
                "Axis {}: local_origin = {}, local_direction = {}",
                i,
                origin,
                dir
            );

            if dir.abs() < 1e-8 {
                log::debug!("Axis {}: Ray is parallel to slab", i);
                if origin < min[i] || origin > max[i] {
                    log::debug!(
                        "Axis {}: Ray origin {} is outside slab bounds ({}, {})",
                        i,
                        origin,
                        min[i],
                        max[i]
                    );
                    return None;
                }
            } else {
                let t1 = (min[i] - origin) / dir;
                let t2 = (max[i] - origin) / dir;
                let (t_near, t_far) = if t1 < t2 { (t1, t2) } else { (t2, t1) };

                log::debug!(
                    "Axis {}: t_near = {}, t_far = {}, before clamp t_min = {}, t_max = {}",
                    i,
                    t_near,
                    t_far,
                    t_min,
                    t_max
                );

                t_min = t_min.max(t_near);
                t_max = t_max.min(t_far);

                log::debug!(
                    "Axis {}: after clamp t_min = {}, t_max = {}",
                    i,
                    t_min,
                    t_max
                );

                if t_min > t_max {
                    log::debug!("Axis {}: No intersection after clamp (t_min > t_max)", i);
                    return None;
                }
            }
        }

        if t_min < 0.0 && t_max < 0.0 {
            log::debug!("OBB intersection is fully behind the ray origin");
            None
        } else {
            let t_result = if t_min >= 0.0 { t_min } else { t_max };
            log::debug!("Intersection at t = {}", t_result);
            Some(t_result)
        }
    }
}
