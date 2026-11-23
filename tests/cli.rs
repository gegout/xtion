use std::process::Command;

#[test]
fn invalid_cli_arguments_fail_before_acquisition() {
    for args in [
        vec!["stream", "0"],
        vec!["stream", "-1"],
        vec!["stream", "NaN"],
        vec!["stream", "3601"],
        vec!["capture", "one", "two"],
        vec!["unknown"],
        vec!["stress", "0"],
        vec!["stress", "1", "101"],
        vec!["recover", "--anything"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtion"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("invalid acquisition state"));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_xtion"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("capture"));
}
