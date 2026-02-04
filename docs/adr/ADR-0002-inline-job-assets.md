# ADR-0002: Inline Job Assets

**Status:** Implemented
**Date:** 2026-02-04
**Authors:** Marcus
**Relates to:** PR #150 (client node ID fix), GitHub Issue #151

## Context

Currently, job code and input are delivered via Iroh blob references:

1. Client encrypts code/input → uploads to local Iroh node
2. Client sends `JobRequest` with blob hashes
3. Worker downloads blobs from Iroh DHT or client node
4. Worker decrypts and executes

This introduces complexity and failure modes:
- Blob download can fail if worker can't reach client (fixed in PR #150)
- Extra round-trip for small payloads
- Iroh DHT adds latency

For most jobs (small code, small input), this overhead is unnecessary.

## Decision

Refactor `JobAssets` to support **per-asset inline or blob delivery**:

### New Types

```rust
pub enum AssetData {
    Inline { data: Vec<u8> },           // Encrypted bytes inline
    Blob { hash: Hash, url: Option<String> },  // Blob reference
}

pub struct JobFile {
    pub path: String,       // Destination path (e.g., "/data/model.bin")
    pub data: AssetData,
}

pub struct JobAssets {
    pub code: AssetData,
    pub input: Option<AssetData>,
    pub files: Vec<JobFile>,            // Additional files
    pub compression: Compression,
}

pub enum Compression { None, Zstd }
```

### Client Options

```typescript
interface AssetOptions {
  mode?: 'auto' | 'inline' | 'blob';
  inlineCodeThreshold?: number;    // default: 4MB
  inlineInputThreshold?: number;   // default: 8MB
  compress?: boolean;
  files?: Record<string, string>;  // dest → src path mapping
}
```

### Mode Behavior

| Mode | Behavior |
|------|----------|
| `auto` (default) | Inline if payload < threshold, blob if larger |
| `inline` | Always inline, error if > 16MB message limit |
| `blob` | Always upload to Iroh (for pre-staging, deduplication) |

### Size Thresholds

- Code: 4 MB default
- Input: 8 MB default
- Total message: 16 MB max (wire protocol limit)

## Alternatives Considered

### 1. Always Inline (Rejected)
**Pros:** Simplest
**Cons:** Large payloads would exceed message limits

### 2. Always Blob (Current)
**Pros:** Works for all sizes
**Cons:** Unnecessary overhead for small jobs, complex failure modes

### 3. Separate Message Types (Rejected)
Different `JobRequest` types for inline vs blob.
**Pros:** Type safety
**Cons:** Explosion of types, mixed mode not possible

## Consequences

### Positive
- Eliminates blob download for common case (small jobs)
- Simpler failure modes
- Lower latency for small payloads
- Files support enables data/model attachments

### Negative
- Breaking wire protocol change
- Workers must handle both modes
- Slightly larger message size for inline mode

### Migration
- Deploy worker changes first (backward compatible - can receive both)
- Update SDK to default to auto mode
- Old clients continue to work (blob mode)

## Implementation

### Files Changed

- `crates/node/src/p2p/protocol/types.rs` - New `AssetData`, `JobFile`, `Compression` types
- `crates/node/src/executor/default.rs` - `fetch_asset()` helper for inline/blob handling
- `crates/node/src/p2p/protocol/handler.rs` - Inline size validation
- `sdks/node/src/types.ts` - TypeScript `AssetOptions` interface
- `sdks/node/native/src/lib.rs` - Updated SDK bindings

## References
- GitHub Issue #151: [Data] Inline job asset delivery
- PR #150: fix(p2p): pass client node ID for direct blob downloads
- Wire protocol: `crates/node/src/p2p/protocol/wire.rs`
- Whitepaper: `docs/WHITEPAPER.md` (Section 4.3 - Inline vs Blob Asset Delivery)
