# rvideo-view

A lightweight viewer for [RVideo](https://crates.io/crates/rvideo) streams.

<img
src="https://raw.githubusercontent.com/roboplc/rvideo/main/rvideo-view/rvideo-view.png"
width="600" />

* Supports all formats supported by RVideo.

* Cross-platform

## Installation

```
cargo install rvideo-view
```

### Usage

```
rvideo-view IP:PORT
```

Additional options:

* --max-fps <MAX_FPS>      [default: 255]
* --timeout <TIMEOUT>      [default: 5]
* --stream-id <STREAM_ID>  [default: 0]

## Metadata display

RVideo allows frame metadata to be encoded in any format. However, to display
frame metadata in rvideo-view, the following requirements must be met:

* Metadata must be encoded in MessagePack format e.g. with
  [rmp-serde](https://crates.io/crates/rmp-serde)].

* To display bounding boxes, they must be in `BoundingBox` structure, provided
  by the [RVideo](https://crates.io/crates/rvideo) crate.

* The bounding boxes array must be placed into `.bboxes` field on top of the
  metadata structure (the structure must be a map).
  [Example](https://github.com/roboplc/rvideo/blob/main/examples/server.rs).
