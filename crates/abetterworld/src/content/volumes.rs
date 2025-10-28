use cgmath::{InnerSpace, Matrix3, Point3, SquareMatrix, Vector3, Zero};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub min: Vector3<f64>,
    pub max: Vector3<f64>,
    pub corners: [Vector3<f64>; 8],
}

impl BoundingBox {
    pub fn default() -> Self {
        BoundingBox {
            min: Vector3::zero(),
            max: Vector3::zero(),
            corners: [Vector3::zero(); 8],
        }
    }
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
    pub fn default() -> Self {
        BoundingVolume {
            bounding_box: [0.0; 12],
        }
    }

    pub fn center(&self) -> Point3<f64> {
        let b = &self.bounding_box;
        Point3::new(b[0], b[1], b[2])
    }

    /// Returns (center, radius) for a covering sphere of the 12-number OBB.
    #[inline]
    pub fn to_bounding_sphere(&self) -> (Point3<f64>, f64) {
        let b = &self.bounding_box;
        let center = Point3::new(b[0], b[1], b[2]);

        // Half-axes (vectors from center to box faces)
        let a0 = Vector3::new(b[3], b[4], b[5]);
        let a1 = Vector3::new(b[6], b[7], b[8]);
        let a2 = Vector3::new(b[9], b[10], b[11]);

        // Radius = sqrt(||a0||^2 + ||a1||^2 + ||a2||^2)
        let r2 = a0.magnitude2() + a1.magnitude2() + a2.magnitude2();
        let radius = r2.sqrt().max(0.0);
        (center, radius)
    }

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
            corners: self.corners(),
        }
    }

    pub fn corners(&self) -> [Vector3<f64>; 8] {
        let obb = self.to_obb();
        let center = obb.center;
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
}
