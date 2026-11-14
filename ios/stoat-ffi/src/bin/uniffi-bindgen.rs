//! UniFFI's bindings generator, built from the same `uniffi` version as the
//! library so the two always match. Run by `scripts/ios/build-core.sh`.

fn main() {
    uniffi::uniffi_bindgen_main()
}
