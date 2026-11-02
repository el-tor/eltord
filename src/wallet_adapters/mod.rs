//! El Tor-specific wallet evidence, dispatch, and recovery adapters.
//! Generic node configuration and APIs remain in LNI; circuit payment policy
//! and its adapter tests live with the daemon that enforces it.
pub mod cln;
pub mod phoenixd;
