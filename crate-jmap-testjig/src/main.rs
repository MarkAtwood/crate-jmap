//! Entry point for the `jmap-testjig` binary.
//!
//! See the crate-level docs in `lib.rs` for the NOT-FOR-PRODUCTION
//! disclaimer. The startup banner and full CLI land in slice
//! `bd:JMAP-cf7p.8`; the actual server wiring lands in slices
//! `bd:JMAP-cf7p.2` through `bd:JMAP-cf7p.7`.

#![forbid(unsafe_code)]

fn main() {
    eprintln!(
        "jmap-testjig: scaffold only — slice bd:JMAP-cf7p.1 \
         landed the crate skeleton; routes / dispatcher wiring / \
         SSE / WS / auth land in subsequent slices. Nothing to do."
    );
}
