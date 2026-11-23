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
        assert_eq!(s.lines().count(), 15);
        assert!(s.contains(if name == "still" {
            "No persistent foreground"
        } else {
            "approaching"
        }));
    }
}
