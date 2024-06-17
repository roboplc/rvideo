# Protocol description

This document describes the RVideo protocol used by the server and the client
to communicate with each other.

* The protocol is binary (TCP-based)

* There is no dedicated port, any one can be used if agreed upon by a client
  and a server

* All numbers are encoded in little-endian

## Data-flow

* Server-to-client: GREETINGS

* Client-to-server: STREAM-SELECT

* Server-to-client: STREAM-INFO

* Server: starts sending frames. To avoid flooding, each frame must be
  acknowledged by the client before the next one is sent.

## Structures

### GREETINGS

(sent by the server)

| B   | Description                 |
| --- | --------------------------- |
| 0   | Hello ("R")                 |
| 1-2 | Number of streams available |

The server supports max 65535 streams registered.

### STREAM-SELECT

(sent by the client)

| B   | Description                 |
| --- | --------------------------- |
| 0-1 | Stream ID                   |
| 2   | FPS limit (mandatory, > 0)  |

The client can request max 255 frames per second.

### STREAM-INFO

(sent by the server)

| B   | Description                 |
|---- | ----------------------------|
| 0-1 | Stream ID                   |
| 2   | Format                      |
| 3-4 | Width                       |
| 5-6 | Height                      |

The max picture size is 65535x65535 pixels.

Formats:

| Value | Description            |
| ----- | ---------------------- |
| 0     | Luma 8-bit             |
| 1     | Luma 16-bit            |
| 2     | Luma 8-bit with alpha  |
| 3     | Luma 16-bit with alpha |
| 4     | RGB 8-bit              |
| 5     | RGB 16-bit             |
| 6     | RGB 8-bit with alpha   |
| 7     | RGB 16-bit with alpha  |
| 64    | MJPEG                  |

* For MJPEG frames can be encoded with any JPEG encoder/parameters and must be
  sent as JPEG images

### Frame

Each frame contains two blocks: metadata and picture data.

### Metadata

| B       | Description                 |
| ------- | --------------------------- |
| 0-3     | Metadata length (0 if none) |
| 4-N     | Metadata (if any)           |

The metadata can be encoded in any way, agreed upon by the client and the
server. For [rvideo-view](https://crates.io/crates/rvideo-view) the metadata
must be encoded in MessagePack.

The max metadata size is `u32::MAX` bytes.

### Picture data

| B       | Description                 |
| ------- | --------------------------- |
| 0-3     | Picture length              |
| 4-N     | Picture data                |

For raw formats, the picture length is always the same, however the protocol
sends the length for each picture data block to allow compressed formats.

The max picture size is `u32::MAX` bytes.

### Acknowledgment

After receiving the frame, the client must send an acknowledgment to the
server. The acknowledgment is a single byte 0x00.
