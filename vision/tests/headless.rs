use std::process::Command;
#[test]
fn fixture_replay_is_repeatable() {
    for name in ["still", "approach"] {
        let run = || {
            Command::new(env!("CARGO_BIN_EXE_xtion-vision"))
                .args([
                    "--headless",
                    "--fast",
                    "--replay",
                    &format!("tests/fixtures/{name}.xtv"),
                ])
                .output()
                .unwrap()
        };
        let a = run();
        assert!(a.status.success());
        assert_eq!(a.stdout, run().stdout);
        let s = String::from_utf8(a.stdout).unwrap();
        assert_eq!(
            s.lines()
                .filter(
                    |line| serde_json::from_str::<serde_json::Value>(line).unwrap()["type"]
                        == "observation"
                )
                .count(),
            15
        );
        assert!(s.contains(if name == "still" {
            "No persistent foreground"
        } else {
            "approaching"
        }));
    }
}

#[test]
fn recorded_semantics_roundtrip_reconstructs_graphs_and_preserves_raw_xtv() {
    use serde_json::Value;
    let dir = std::env::temp_dir().join(format!(
        "xtion-semantic-test-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("session.xtv");
    let bin = env!("CARGO_BIN_EXE_xtion-vision");
    let run = Command::new(bin)
        .args([
            "--headless",
            "--fast",
            "--replay",
            "tests/fixtures/approach.xtv",
            "--record",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(run.status.success());
    let again = Command::new(bin)
        .arg("--play-semantics")
        .arg(path.with_extension("semantic.jsonl"))
        .output()
        .unwrap();
    assert!(again.status.success());
    let rows = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .filter_map(|p| p.get("semantic").cloned())
            .collect::<Vec<_>>()
    };
    let original = rows(&run.stdout);
    let recorded = rows(&again.stdout);
    assert!(recorded.len() > 1);
    for g in recorded {
        assert!(original.contains(&g));
    }
    // XTV permits whitespace in its JSON header. Compare every header field and every raw sample.
    use xtion_vision::source::{FrameSource, ReplaySource};
    let open = |p: &std::path::Path| {
        ReplaySource::new(
            std::io::BufReader::new(std::fs::File::open(p).unwrap()),
            false,
        )
        .unwrap()
    };
    let (mut before, mut after) = (open("tests/fixtures/approach.xtv".as_ref()), open(&path));
    let mut n = 0;
    loop {
        let a = before.next_frame().unwrap();
        let b = after.next_frame().unwrap();
        assert_eq!(a, b);
        if a.is_none() {
            break;
        }
        n += 1;
    }
    assert_eq!(n, 15);
    std::fs::remove_dir_all(dir).unwrap();
}
