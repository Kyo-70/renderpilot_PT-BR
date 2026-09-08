//! Peer topology state is committed only by `PeerStorageRuntime`.
//!
//! This module intentionally contains no storage-owned route, endpoint, or
//! receipt DTO. The domain peer-transition types are the sole program
//! vocabulary; runtime ownership and CAS live in `repositories::peer_runtime`.
