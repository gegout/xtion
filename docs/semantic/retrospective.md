# Xtion Vision 0.3 retrospective

Version 0.2 established what each visible region measured. Version 0.3 adds a session-local account of which observations appear to belong together, how their positions relate and which changes persist. Each conclusion points back to its evidence. A reply about distance comes directly from the graph, whether the model is enabled or the application runs offline.

## What the experiments changed

The saved empty-room reference looked promising for a scene that already contained a subject at startup. On the relocated recording it reduced dominant-region frames by 277. It remains available in the comparison example. Production segmentation retained the V0.2 counts and outline scores. Flagging gaps before counting motion votes adds a guard against a false event at a capture discontinuity. [Joint comparison](joint-replay.json) · [Outline scores](mask-regression.json)

The sidecar test exposed a last-decimal change during floating-point JSON decoding. Round-trip float parsing now preserves the recorded geometry. A second compatibility test compares every decoded header field and raw pixel in an old XTV recording after a read/write cycle. The audit distinguishes a redraw submission, a new sensor frame submitted for display and a physical compositor presentation. Only the first two are instrumented.

## What the evidence supports

An offline consumer can ask about tracked entities, measured distance, movement and recent events. The application can mark the same entity on a depth image and a shaded surface and reject a model reply with a fabricated measurement or evidence ID. A model label carries its own provenance and expires when the supporting observations disappear.

A script cue provides an instruction. Reviewed recordings are required before assigning accuracy to statements about a human. No independent reviewed human evaluation set or consented endpoint was available. Sitting, standing posture, arm motion, reaching, absolute lateral accuracy and comparative model quality remain unverified. The ten-minute sensor run qualifies acquisition and processing on this laptop. [Validation report](release.md)
