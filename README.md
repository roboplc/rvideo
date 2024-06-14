<h2>
  RVideo
  <a href="https://crates.io/crates/rvideo"><img alt="crates.io page" src="https://img.shields.io/crates/v/rvideo.svg"></img></a>
  <a href="https://docs.rs/rvideo"><img alt="docs.rs page" src="https://docs.rs/rvideo/badge.svg"></img></a>
</h2>

Real-time video server for embedded apps.

## What is RVideo

RVideo is a library which solves the problem of streaming video from embedded
computer-vision applications. Many of such are headless and do not require a
dedicated interface, however it is often useful (especially for developers) to
see what is happening on the device. RVideo provides a simple API to stream
video from your embedded application to a remote client.

## How does it work

Unlike other streaming solutions, the goal of RVideo is to provide a minimal
overhead for an embedded application it is included into:

* Frames are always sent as-is, usually in RAW formats (it is more than enough
  for most debugging use-cases)

* All frames, not received by a client in time, are dropped

* No any buffering is performed on the server side

* Real-time-safe code is used to minimize the impact on the main application

## Clients

RVideo streams can be received with clients provided by crate. For ready-to-use
UI, see the [`rvideo-view`](https://crates.io/crates/rvideo-view) crate.

## About

RVideo is a part of [RoboPLC](https://www.roboplc.com/) project.
