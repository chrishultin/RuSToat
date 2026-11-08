//! Link flags the app binary needs.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // libwebrtc (through the `livekit` crate) ships Objective-C categories.
    // Without `-ObjC` the linker drops them and the app aborts at startup with
    // "unrecognized selector" (livekit/rust-sdks#795). webrtc-sys's own
    // `rustc-link-arg` doesn't reach our binary, so pass it here. Only to the
    // app binary: the integration tests never touch WebRTC.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bins=-ObjC");
    }
}
