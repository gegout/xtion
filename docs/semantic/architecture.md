# Intelligent Spatial Perception architecture

An immutable spatial observation feeds one session-local semantic engine. Live, replay and synthetic inputs share it. The engine imports geometry and observation types and contains no UI, USB or HTTP operations. The session namespace is derived from the first frame. Capture health, optical calibration source and host monotonic times accompany the interpretations.

## From tracks to evidence

Each observed entity retains its member track IDs and up to 32 historical fragment IDs. Nearby fragments need compatible nonzero motion over three observations before association. Complete-link grouping keeps one bridging fragment from joining two unrelated groups. Short losses can be matched by motion extrapolation. Divergent fragments split. Relations identify the sensor optical frame in metres, X right, Y down and Z forward. Position, velocity and bounds describe visible samples. Axial noise is modelled and lateral accuracy inherits optical calibration quality.

The graph schema is `xtion.semantic/1`. Source observation references carry measured provenance. Aggregated bounds, relationships and motion intervals carry derived provenance. Entry and exit are hypotheses about visibility. Each lists occlusion or tracking ambiguity. A support-plane estimate has no room identity until the operator identifies the surface. Labels expire or lose validity on fragmentation and unreliable geometry. A stable AI label requires three distinct accepted observations. Its self-assessed confidence supplies no probability of correctness.

## Consumers

The desktop uses the graph paired with the displayed frame. The mesh viewer retains a bounded lookup of matching graphs while the asynchronous reconstruction worker finishes. The camera can orbit around the measured surface. Turning off entity, activity or relationship marks affects only display. Dialogue resolves numeric answers from the local graph. The optional model can invoke the local scene query or select a verbatim locally computed answer. Operator questions take queue priority. Activity queries use configurable bounded time windows.

AI scheduling keeps one request in flight and one pending job. Entity changes outrank activity changes, retries and refreshes. Every HTTP attempt consumes request budget, including probes, retries and transport fallbacks. Admission reserves request bytes plus the output cap as a conservative estimate. Reported token totals replace the reservation when available. Current JSON, marked depth and sensor-facing shading come from the same snapshot. Unsupported claims reject the reply atomically. Counters distinguish HTTP attempts, tokens, claims seen, claims withheld, and reply rejection category. Unknown posture remains explicit.

## Recording and reproducibility

XTV1 preserves raw frames. A local JSONL sidecar periodically writes a full graph and writes deltas between checkpoints. Each record carries its capture timestamp, source optics, activities, admitted report, completed AI request metadata and writer-drop count. Disk work uses a bounded drop-oldest queue. Replaying a saved interpretation requires an exact timestamp match. Missing interpretations have an explicit status. Headless playback can read the saved graphs without opening a sensor or contacting an endpoint.

## Research scope

ConceptGraphs combines posed RGB-D views and RGB models into a map. Xtion uses session-local object nodes and evidence edges. Set-of-Mark studied visual grounding in RGB images. Xtion draws local entity IDs on depth and shaded surfaces. SpatialBot and SpatialRGPT use richer training data and model architectures. Their results motivate testing numeric measurements and region references here. No performance result from those papers was transferred to this sensor. [ConceptGraphs](https://arxiv.org/html/2309.16650) · [Set-of-Mark](https://arxiv.org/html/2310.11441v2) · [SpatialBot](https://arxiv.org/html/2406.13642v2) · [SpatialRGPT](https://arxiv.org/abs/2406.01584)

The implementation uses the project's existing Rust dependencies. No model code, weights or external depth dataset were distributed. The geodesic-extrema and learned skeleton experiments were omitted. No patent clearance is claimed for third-party methods.
