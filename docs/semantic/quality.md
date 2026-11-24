# Observation quality gate

The production motion voter now requires a tracking frame before counting a velocity as evidence. The UI asks the operator to identify a support surface before accepting it as the floor. All seven recordings matched the protected tracking and event counts exactly. No foreground membership or association rule changed. The historical outlines come from the development recordings. There are zero new independently reviewed human clips. [Joint comparison](joint-replay.json)

As an experiment the replay example accepts an empty reference XTV as its third input after the frame offset. It learns the first five frames and resets the temporal state before scoring. On the relocated occupied recording dominance fell from 1,347 to 1,070 of 1,347 frames. This option stays in the comparison tool. [Experiment](empty-reference-experiment.json)

The five-minute XTV metadata supports counting runs of consecutive arrivals with increased gap, incomplete or decoder counters. Counters describe complete frames delivered after the source detected loss. The file has no synchronised CPU load trace for causal attribution. [Counter analysis](health.json)

The PS1080 stream loads zero-plane distance and pixel size from fixed parameters. Its pixel scaling also depends on firmware reference resolution. The Rust driver already reads both fixed values at offsets 100 and 104 for its axial lookup table. Its application API exposes the calibrated depth. Nominal lateral optics are retained pending independent validation with a known-size object. No change to the driver or device protocol was necessary. [OpenNI fixed parameters and DecidePixelSizeFactor](https://github.com/OpenNI/OpenNI2/blob/115cf06c6efea32304182d293eca16ca883c9150/Source/Drivers/PS1080/Sensor/XnSensorDepthStream.cpp)

The eight-second desktop replay exceeded 30 submissions/s on glow. The release's five-minute visible and isolated-display tests had different concurrent loads. Compositor presentation timestamps are unavailable in the viewer. Visible long-run presentation performance remains open.
