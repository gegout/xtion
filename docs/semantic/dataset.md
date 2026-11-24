# Local cue collection

Run `cargo build --release -p xtion-vision --example guided_capture`. In a visible desktop terminal run `target/release/examples/guided_capture capture recordings/NEW_SESSION`. It records ten scenarios twice into separate XTV files and plays a desktop sound at cue onset. Use the terminal zoom control to size the cues for the operator. Keep the camera fixed throughout. An object reference requires independently measured dimensions.

Each cue has a local JSON file with the first and last included frame, both host times in nanoseconds, and a one-second edge tolerance. The manifest fixes the split by recording and hashes the XTV bytes. A cue starts as instructed. Review with `target/release/examples/guided_capture review RECORDING.xtv` from a desktop terminal with ffplay available. Enter advances through depth PNGs. Accept the interval or supply a corrected label and inclusive frame bounds. The reviewed sidecar preserves the original instruction.

The synthetic exercise produced 20 instructed files, ten per split. One review exercised corrected frame bounds. Synthetic files use repeated generated content and are excluded from human accuracy evaluation. No reviewed human recordings or known dimensions were supplied at this gate. [Synthetic exercise manifest](synthetic-manifest.json)
