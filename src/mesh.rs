use {normal_from_object, BoundingBox, Object};
use alga::general::Real;
use na;
use num_traits::Float;
use std::fmt::Debug;


#[derive(Clone, Debug, PartialEq)]
struct Face<S: Real + Debug> {
    normal: na::Vector3<S>,
    vertices: [usize; 3],
}

/// Mesh generates an implicit function from a 3d object mesh.
/// Warning! This primitive is currently horribly inefficient.
/// That is, for each point it iterates over all faces and finds the closest.
/// This implementation desperately needs some performance improvements, e.g. kd-tree support or
/// similar.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh<S: Real + Debug> {
    bbox: BoundingBox<S>,
    vertices: Vec<na::Vector3<S>>,
    faces: Vec<Face<S>>,
}

impl<S: Debug + Real + Float + From<f64> + From<f32>> Mesh<S> {
    /// Create a new Mesh from a [STL file](https://en.wikipedia.org/wiki/STL_(file_format)).
    pub fn new(stl_filename: &str) -> ::std::io::Result<Box<Mesh<S>>> {
        let mut file = ::std::fs::OpenOptions::new().read(true).open(stl_filename)?;
        let mesh = ::stl_io::read_stl(&mut file)?;
        mesh.validate()?;
        let vertices = mesh.vertices
            .iter()
            .map(|v| {
                na::Vector3::new(From::from(v[0]), From::from(v[1]), From::from(v[2]))
            })
            .collect::<Vec<_>>();
        let faces = mesh.faces
            .iter()
            .map(|f| {
                let n = (vertices[f.vertices[1]] - vertices[f.vertices[0]])
                    .cross(&(vertices[f.vertices[2]] - vertices[f.vertices[0]]))
                    .normalize();
                Face {
                    normal: n,
                    vertices: f.vertices,
                }
            })
            .collect::<Vec<_>>();
        let bbox = bbox_for_mesh(&mesh);
        Ok(Box::new(Mesh {
            bbox: bbox,
            vertices: vertices,
            faces: faces,
        }))
    }
    fn value(&self, p: na::Point3<S>) -> S {
        let p = na::Vector3::new(p.x, p.y, p.z);
        let value_and_acos = self.faces.iter().fold(
            (Float::max_value(), From::from(0f64)),
            |min_and_acos, f| {
                let current_and_acos = distance_point_face(
                    [
                        &self.vertices[f.vertices[0]],
                        &self.vertices[f.vertices[1]],
                        &self.vertices[f.vertices[2]],
                    ],
                    &f.normal,
                    &p,
                );
                if current_and_acos.0.relative_eq(
                    &min_and_acos.0,
                    S::default_epsilon(),
                    S::default_max_relative(),
                ) {
                    // current_and_acos.0 == min_and_acos.0
                    let mut best_acos: S = min_and_acos.1;
                    if Float::abs(current_and_acos.1) > Float::abs(best_acos) {
                        best_acos = current_and_acos.1;
                    }
                    return (min_and_acos.0, best_acos);
                }
                if current_and_acos.0 < min_and_acos.0 {
                    return current_and_acos;
                } else {
                    return min_and_acos;
                }
            },
        );
        return value_and_acos.0 * Float::signum(value_and_acos.1);
    }
}

// Project p onto line ab. Return None, if the projection would not fall between a and b.
fn point_over_line<S: Debug + Real + From<f64>>(
    a: &na::Vector3<S>,
    b: &na::Vector3<S>,
    p: &na::Vector3<S>,
) -> Option<na::Vector3<S>> {
    let ab = b - a;
    let ap = p - a;
    let scale = ap.dot(&ab) / ab.dot(&ab);
    if scale < From::from(0f64) || scale > From::from(1f64) {
        return None;
    }
    return Some(a + ab * scale);
}

// Project p onto plane of triangle. Return None, if the projection would not fall into the
// triangle.
// Triangle is defined via points a,b,c and normal n.
fn point_over_triangle<S: Debug + Real + Float + From<f64>>(
    a: &na::Vector3<S>,
    b: &na::Vector3<S>,
    c: &na::Vector3<S>,
    n: &na::Vector3<S>,
    p: &na::Vector3<S>,
) -> Option<na::Vector3<S>> {
    let _0: S = From::from(0f64);
    let _1: S = From::from(1f64);

    let proj = p - n * (p - a).dot(n);

    // The vector ab and bc span the triangle.
    let ab = b - a;
    let bc = c - b;

    // Vector from a to projected point.
    let aproj = proj - a;

    // find linear combination of ab and bc to aproj:
    // aproj = k * ab + l * bc
    // This is the basic formular for l. But the denominator can be zero for certain cases.
    // let l = (aproj.x * ab.y - aproj.y * ab.x) / (bc.x * ab.y - bc.y * ab.x);
    let l;
    let mut ld = bc.x * ab.y - bc.y * ab.x;
    if ld != _0 {
        l = (aproj.x * ab.y - aproj.y * ab.x) / ld;
    } else {
        ld = bc.x * ab.z - bc.z * ab.x;
        if ld != _0 {
            l = (aproj.x * ab.z - aproj.z * ab.x) / ld;
        } else {
            ld = bc.z * ab.y - bc.y * ab.z;
            debug_assert!(ld != _0);
            l = (aproj.z * ab.y - aproj.y * ab.z) / ld;
        }
    }
    let k;
    if ab.x != _0 {
        k = (aproj.x - l * bc.x) / ab.x;
    } else if ab.y != _0 {
        k = (aproj.y - l * bc.y) / ab.y;
    } else {
        k = (aproj.z - l * bc.z) / ab.z;
    }

    if k < _0 || l < _0 || k > _1 || l > k {
        return None;
    }

    return Some(proj);
}

// Assumes that a and b are parallel.
// returns 1 if a and b point in the same direction.
// returns -1 if a and b point in opposite directions.
fn vector_direction<S: Debug + Real + From<f64> + Float>(
    a: &na::Vector3<S>,
    b: &na::Vector3<S>,
) -> S {
    let _0: S = From::from(0f64);
    let _1: S = From::from(1f64);
    for i in 0..a.len() {
        if a[i] != _0 {
            if Float::signum(a[i]) == Float::signum(b[i]) {
                return _1;
            } else {
                return -_1;
            }
        }
    }
    // a is a zero-vector the sign direction does not matter. Still return 1, to make sure we have
    // a valid value.
    return _1;
}

// Returns the distance between p and the triangle face (first value).
// The second value is the acos of the angle between the normal of face and the line from p to
//  the closest point of face.
fn distance_point_face<S: Debug + Real + From<f64> + Float>(
    face: [&na::Vector3<S>; 3],
    n: &na::Vector3<S>,
    p: &na::Vector3<S>,
) -> (S, S) {
    if let Some(proj) = point_over_triangle(face[0], face[1], face[2], n, p) {
        let delta = p - proj;
        return (delta.norm(), vector_direction(&delta, n));
    }

    let _0: S = From::from(0f64);

    // Iterate over all edges to find any closest projection.
    let mut closest_point_and_dist = [(face[0], face[1]), (face[1], face[2]), (face[2], face[0])]
        .iter()
        .fold(
            (na::Vector3::new(_0, _0, _0), S::infinity()),
            |best_point_and_dist, line| {
                let optional_point = point_over_line(line.0, line.1, &p);
                if let Some(ref pp) = optional_point {
                    let vector_to_egde = p - pp;
                    let current_dist = vector_to_egde.norm();
                    if current_dist < best_point_and_dist.1 {
                        return (*pp, current_dist);
                    }
                }
                return best_point_and_dist;
            },
        );

    // Now also iterate over all vertices to find a point that might even be closer.
    closest_point_and_dist =
        face.iter()
            .fold(closest_point_and_dist, |best_point_and_dist, vertex| {
                let vector_to_vertex = p - *vertex;
                let current_dist = vector_to_vertex.norm();
                if current_dist < best_point_and_dist.1 {
                    return (**vertex, current_dist);
                }
                return best_point_and_dist;
            });

    assert!(closest_point_and_dist.1 < S::infinity());

    let vector_to_point = p - closest_point_and_dist.0;
    return (
        closest_point_and_dist.1,
        vector_to_point.dot(n) / closest_point_and_dist.1,
    );
}

fn bbox_for_mesh<S: Real + From<f32> + Float>(mesh: &::stl_io::IndexedMesh) -> BoundingBox<S> {
    mesh.vertices
        .iter()
        .fold(BoundingBox::neg_infinity(), |bbox, v| {
            bbox.insert(na::Point3::new(
                From::from(v[0]),
                From::from(v[1]),
                From::from(v[2]),
            ))
        })
}

impl<S: Real + Float + From<f64> + From<f32>> Object<S> for Mesh<S> {
    fn approx_value(&self, p: na::Point3<S>, slack: S) -> S {
        let approx = self.bbox.distance(p);
        if approx <= slack {
            self.value(p)
        } else {
            approx
        }
    }
    fn bbox(&self) -> &BoundingBox<S> {
        &self.bbox
    }
    fn normal(&self, p: na::Point3<S>) -> na::Vector3<S> {
        normal_from_object(self, p)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_point_over_line() {
        let o = na::Vector3::new(0., 0., 0.);
        let d = na::Vector3::new(10., 10., 10.);
        assert_eq!(
            point_over_line(&o, &d, &na::Vector3::new(-1., 0., 0.)),
            None
        );
        assert_eq!(point_over_line(&o, &d, &o), Some(o.clone()));
        assert_eq!(point_over_line(&o, &d, &d), Some(d.clone()));
        assert!(point_over_line(&o, &d, &na::Vector3::new(5., 3., 0.)).is_some());
        assert_eq!(
            point_over_line(&o, &d, &na::Vector3::new(-5., 3., 0.)),
            None
        );
    }

    #[test]
    fn test_point_over_triangle() {
        let a = na::Vector3::new(0., 0., 10.);
        let b = na::Vector3::new(0., 0., -10.);
        let c = na::Vector3::new(0., 10., 0.);
        let n = na::Vector3::new(-1., 0., 0.);
        assert_eq!(point_over_triangle(&a, &b, &c, &n, &a), Some(a.clone()));
        assert_eq!(point_over_triangle(&a, &b, &c, &n, &b), Some(b.clone()));
        assert_eq!(point_over_triangle(&a, &b, &c, &n, &c), Some(c.clone()));

        assert_eq!(
            point_over_triangle(&a, &b, &c, &n, &na::Vector3::new(5., 1., 0.)),
            Some(na::Vector3::new(0., 1., 0.))
        );
        assert_eq!(
            point_over_triangle(&a, &b, &c, &n, &na::Vector3::new(-5., 1., 0.)),
            Some(na::Vector3::new(0., 1., 0.))
        );
        assert_eq!(
            point_over_triangle(&a, &b, &c, &n, &na::Vector3::new(5., 0., 0.)),
            Some(na::Vector3::new(0., 0., 0.))
        );
        assert_eq!(
            point_over_triangle(&a, &b, &c, &n, &na::Vector3::new(-5., 0., 0.)),
            Some(na::Vector3::new(0., 0., 0.))
        );
        assert_eq!(
            point_over_triangle(&a, &b, &c, &n, &na::Vector3::new(5., -1., 0.)),
            None
        );
        assert_eq!(
            point_over_triangle(&a, &b, &c, &n, &na::Vector3::new(-5., -1., 0.)),
            None
        );
    }

    #[test]
    fn test_distance_point_face() {
        let a = na::Vector3::new(0., 0., 0.);
        let b = na::Vector3::new(10., 0., 0.);
        let c = na::Vector3::new(0., 10., 0.);
        let face = [&a, &b, &c];
        let n = b.cross(&c).normalize();
        assert_eq!(distance_point_face(face.clone(), &n, &a), (0., 1.));
        assert_eq!(distance_point_face(face.clone(), &n, &b), (0., 1.));
        assert_eq!(distance_point_face(face.clone(), &n, &c), (0., 1.));
        assert_eq!(
            distance_point_face(face.clone(), &n, &na::Vector3::new(-10., 0., 0.)),
            (10., 0.)
        );
        assert_eq!(
            distance_point_face(face.clone(), &n, &na::Vector3::new(1., 1., 10.)),
            (10., 1.)
        );
        assert_eq!(
            distance_point_face(face.clone(), &n, &na::Vector3::new(1., 1., -10.)),
            (10., -1.)
        );

        assert!(distance_point_face(face.clone(), &n, &na::Vector3::new(-1., -1., 10.)).0 > 10.);
        assert!(distance_point_face(face.clone(), &n, &na::Vector3::new(-1., -1., 10.)).1 > 0.);

        assert!(distance_point_face(face.clone(), &n, &na::Vector3::new(-1., -1., -10.)).0 > 10.);
        assert!(distance_point_face(face.clone(), &n, &na::Vector3::new(-1., -1., -10.)).1 < 0.);
    }

    #[test]
    fn test_distance_point_face_by_halfcircle_around_face_edge() {
        let a = na::Vector3::new(0., 0., 1.);
        let b = na::Vector3::new(0., 0., -1.);
        let c = na::Vector3::new(0., -1., 0.);
        let face = [&a, &b, &c];
        let n = na::Vector3::new(-1., 0., 0.);

        let steps = 100;
        let dist = 100.0;
        for i in 0..steps {
            let angle = i as f64 * ::std::f64::consts::PI / steps as f64;
            let x = -angle.cos() * dist;
            let y = angle.sin() * dist;
            let p = na::Vector3::new(x, y, 0.);
            let result = distance_point_face(face.clone(), &n, &p);
            assert_ulps_eq!(result.0, dist);
            assert_ulps_eq!(result.1, angle.cos());
        }
    }

    #[test]
    fn test_distance_point_face_by_halfcircle_around_face_point() {
        let a = na::Vector3::new(0., -1., 1.);
        let b = na::Vector3::new(0., -1., -1.);
        let c = na::Vector3::new(0., 0., 0.);
        let face = [&a, &b, &c];
        let n = na::Vector3::new(-1., 0., 0.);

        let steps = 10;
        let dist = 100.0;
        for i in 0..steps {
            let angle = i as f64 * ::std::f64::consts::PI / steps as f64;
            let x = -angle.cos() * dist;
            let y = angle.sin() * dist;
            let p = na::Vector3::new(x, y, 0.);
            let result = distance_point_face(face.clone(), &n, &p);
            assert_ulps_eq!(result.0, dist);
            assert_ulps_eq!(result.1, angle.cos());
        }
    }

    #[test]
    fn test_2face_edge() {
        let convex_mesh = Mesh {
            bbox: BoundingBox::<f64>::infinity(),
            vertices: vec![
                na::Vector3::new(0., 0., 100.),
                na::Vector3::new(0., 0., -100.),
                na::Vector3::new(100., -100., 0.),
                na::Vector3::new(-100., -100., 0.),
            ],
            faces: vec![
                Face {
                    normal: na::Vector3::new(1., 1., 0.).normalize(),
                    vertices: [0, 1, 2],
                },
                Face {
                    normal: na::Vector3::new(-1., 1., 0.).normalize(),
                    vertices: [1, 0, 3],
                },
            ],
        };
        let mut concave_mesh = convex_mesh.clone();
        concave_mesh.faces = vec![
            Face {
                normal: na::Vector3::new(-1., -1., 0.).normalize(),
                vertices: [0, 2, 1],
            },
            Face {
                normal: na::Vector3::new(1., -1., 0.).normalize(),
                vertices: [1, 3, 0],
            },
        ];
        let steps = 10;
        for i in 0..steps {
            for &(mesh, sign) in [(&convex_mesh, 1.), (&concave_mesh, -1.)].iter() {
                let x = i as f64 / steps as f64;

                let outside1 = na::Point3::new(x, 0., 0.);
                let outside2 = na::Point3::new(-x, 0., 0.);

                let expected_outside_dist = sign * x / 2f64.sqrt();

                assert_ulps_eq!(mesh.approx_value(outside1, 0.), expected_outside_dist);
                assert_ulps_eq!(mesh.approx_value(outside2, 0.), expected_outside_dist);


                let infront = na::Point3::new(0.5 - x, 1., 0.);
                let infront_dist = sign * na::Vector3::new(0.5 - x, 1., 0.).norm();
                assert_ulps_eq!(mesh.approx_value(infront, 0.), infront_dist);

                let inside1 = na::Point3::new(1.0 - x, -1.0 - x, 0.);
                let inside2 = na::Point3::new(-1.0 + x, -1.0 - x, 0.);

                let expected_inside_dist = sign * -x * 2f64.sqrt();

                assert_ulps_eq!(mesh.approx_value(inside1, 0.), expected_inside_dist);
                assert_ulps_eq!(mesh.approx_value(inside2, 0.), expected_inside_dist);
            }
        }
    }

    #[test]
    fn test_2face_convex_vertex() {
        let mesh = Mesh {
            bbox: BoundingBox::<f64>::infinity(),
            vertices: vec![
                na::Vector3::new(0., 0., 0.),
                na::Vector3::new(100., -100., -100.),
                na::Vector3::new(100., -100., 100.),
                na::Vector3::new(-100., -100., -100.),
                na::Vector3::new(-100., -100., 100.),
            ],
            faces: vec![
                Face {
                    normal: na::Vector3::new(1., 1., 0.).normalize(),
                    vertices: [0, 1, 2],
                },
                Face {
                    normal: na::Vector3::new(-1., 1., 0.).normalize(),
                    vertices: [0, 4, 3],
                },
            ],
        };
        let steps = 10;
        for i in 0..steps {
            let x = i as f64 / steps as f64;

            let p1 = na::Point3::new(x, 0., 0.);
            let p2 = na::Point3::new(-x, 0., 0.);

            let expected_dist = x / 2f64.sqrt();

            assert_ulps_eq!(mesh.approx_value(p1, 0.), expected_dist);
            assert_ulps_eq!(mesh.approx_value(p2, 0.), expected_dist);
        }
    }

    #[test]
    fn test_2face_concave_vertex() {
        let mesh = Mesh {
            bbox: BoundingBox::<f64>::infinity(),
            vertices: vec![
                na::Vector3::new(0., 0., 0.),
                na::Vector3::new(100., 100., 100.),
                na::Vector3::new(100., 100., -100.),
                na::Vector3::new(-100., 100., 100.),
                na::Vector3::new(-100., 100., -100.),
            ],
            faces: vec![
                Face {
                    normal: na::Vector3::new(-1., 1., 0.).normalize(),
                    vertices: [0, 1, 2],
                },
                Face {
                    normal: na::Vector3::new(1., 1., 0.).normalize(),
                    vertices: [0, 4, 3],
                },
            ],
        };
        let steps = 10;
        for i in 0..steps {
            let x = i as f64 / steps as f64;

            let p1 = na::Point3::new(x, 2. - x, 0.);
            let p2 = na::Point3::new(-x, 2. - x, 0.);

            let expected_dist = (1.0 - x) * 2f64.sqrt();

            assert_ulps_eq!(mesh.approx_value(p1, 0.), expected_dist);
            assert_ulps_eq!(mesh.approx_value(p2, 0.), expected_dist);
        }
    }
}
