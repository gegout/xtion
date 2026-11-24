//! Portable single-frame exports. The whole directory shares scene.json metadata.
use crate::{
    DepthFrame,
    geometry::Transform,
    perception::{NoiseModel, SceneState},
    reconstruction::{MeshSettings, SurfaceMesh},
    source::Result,
};
use serde_json::json;
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};

pub fn export(
    dir: &Path,
    frame: &DepthFrame,
    scene: &SceneState,
    pose: Transform,
    settings: MeshSettings,
) -> Result<()> {
    let cloud = frame
        .organised_cloud()
        .ok_or("Metric depth and known intrinsics required for geometry export")?;
    fs::create_dir(dir)?;
    let write = || -> Result<()> {
        let metadata = json!({"schema":"xtion.spatial/0.2","units":"metres","coordinate_frame":"camera optical: X right, Y down, Z forward","pose":pose,"frame":frame.info,"data":"raw","mesh_stride":settings.stride,"mesh_erosion":settings.erode});
        let file = |name: &str| -> std::io::Result<_> {
            Ok(BufWriter::new(File::create_new(dir.join(name))?))
        };
        let mut state = file("scene.json")?;
        serde_json::to_writer(&mut state, &json!({"metadata":metadata,"scene":scene}))?;
        state.flush()?;
        let n = frame.pixels.iter().filter(|&&d| d != 0).count();
        for (name, format) in [
            ("points.ply", "ascii"),
            ("points-binary.ply", "binary_little_endian"),
        ] {
            let mut w = file(name)?;
            writeln!(
                w,
                "ply\nformat {format} 1.0\ncomment {metadata}\nelement vertex {n}\nproperty double x\nproperty double y\nproperty double z\nend_header"
            )?;
            for (_, p) in cloud.samples(std::num::NonZeroUsize::MIN) {
                if format == "ascii" {
                    writeln!(w, "{} {} {}", p.x, p.y, p.z)?;
                } else {
                    for c in [p.x, p.y, p.z] {
                        w.write_all(&c.to_le_bytes())?;
                    }
                }
            }
            w.flush()?;
        }
        let mut mesh = SurfaceMesh::default();
        mesh.update(frame, settings, &NoiseModel::default());
        let mut obj = file("surface.obj")?;
        let mut ply = file("surface.ply")?;
        writeln!(obj, "# {metadata}")?;
        writeln!(
            ply,
            "ply\nformat ascii 1.0\ncomment {metadata}\nelement vertex {}\nproperty double x\nproperty double y\nproperty double z\nproperty double nx\nproperty double ny\nproperty double nz\nelement face {}\nproperty list uchar int vertex_indices\nend_header",
            mesh.vertices.len(),
            mesh.triangles.len()
        )?;
        for v in &mesh.vertices {
            let p = v.point;
            let [x, y, z] = v.normal;
            writeln!(ply, "{} {} {} {x} {y} {z}", p.x, p.y, p.z)?;
            writeln!(obj, "v {} {} {}\nvn {x} {y} {z}", p.x, p.y, p.z)?;
        }
        for &[a, b, c] in &mesh.triangles {
            writeln!(ply, "3 {a} {b} {c}")?;
            let (a, b, c) = (a + 1, b + 1, c + 1);
            writeln!(obj, "f {a}//{a} {b}//{b} {c}//{c}")?;
        }
        obj.flush()?;
        ply.flush()?;
        let mut csv = file("trajectories.csv")?;
        writeln!(csv, "track_id,arrival_ns,x_m,y_m,z_m")?;
        let mut tracks = vec![];
        for o in &scene.objects {
            tracks.push(json!({"track":o.id,"points":o.trajectory}));
            for (ns, [x, y, z]) in &o.trajectory {
                writeln!(csv, "{},{ns},{x},{y},{z}", o.id)?;
            }
        }
        csv.flush()?;
        let mut traj = file("trajectories.json")?;
        serde_json::to_writer(&mut traj, &json!({"metadata":metadata,"tracks":tracks}))?;
        traj.flush()?;
        let (w, h, rgb) = crate::image::rgb(frame, frame.info.width, 4000);
        fs::write(dir.join("depth.png"), crate::image::png(w, h, &rgb)?)?;
        Ok(())
    };
    let result = write();
    if result.is_err() {
        let _ = fs::remove_dir_all(dir);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        perception::Engine,
        source::{FrameSource, SyntheticSource},
    };
    #[test]
    fn bundle_roundtrips_cloud_faces_and_trajectory_and_preserves_existing_output() {
        let mut src = SyntheticSource::default();
        let mut e = Engine::default();
        let (f, s) = (0..20)
            .map(|_| {
                let f = src.next_frame().unwrap().unwrap();
                let s = e.observe(&f);
                (f, s)
            })
            .last()
            .unwrap();
        let dir =
            std::env::temp_dir().join(format!("xtion-export-{}-{}", std::process::id(), s.ns));
        export(&dir, &f, &s, Transform::IDENTITY, Default::default()).unwrap();
        assert!(export(&dir, &f, &s, Transform::IDENTITY, Default::default()).is_err());
        let text = |name: &str| fs::read_to_string(dir.join(name)).unwrap();
        let value = |name: &str| serde_json::from_str::<serde_json::Value>(&text(name)).unwrap();
        assert_eq!(
            value("scene.json")["metadata"]["frame"]["ticks"],
            f.info.ticks
        );
        let pts = text("points.ply");
        let binary = fs::read(dir.join("points-binary.ply")).unwrap();
        let prefix = b"end_header\n";
        let off = binary
            .windows(prefix.len())
            .position(|s| s == prefix)
            .unwrap()
            + prefix.len();
        let floats = binary[off..]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|n| f64::from_le_bytes(*n))
            .collect::<Vec<_>>();
        let ascii: Vec<f64> = pts
            .split_once("end_header\n")
            .unwrap()
            .1
            .split_whitespace()
            .map(|n| n.parse().unwrap())
            .collect();
        assert_eq!(floats, ascii);
        assert_eq!(
            floats.len(),
            3 * f.pixels.iter().filter(|&&d| d != 0).count()
        );
        let obj = text("surface.obj");
        let nf = obj.lines().filter(|l| l.starts_with("f ")).count();
        assert!(nf > 0);
        assert!(text("surface.ply").contains(&format!("element face {nf}\n")));
        let tracks = value("trajectories.json");
        assert_eq!(tracks["tracks"][0]["track"], s.objects[0].id);
        assert_eq!(
            text("trajectories.csv").lines().count() - 1,
            s.objects.iter().map(|o| o.trajectory.len()).sum::<usize>()
        );
        assert!(
            fs::read(dir.join("depth.png"))
                .unwrap()
                .starts_with(b"\x89PNG")
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
