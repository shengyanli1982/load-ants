# Metrics Label Cardinality Fix Implementation Plan

> **For agentic workers:** REQUIRED: Use `plan-runbook-execute` for development execution and add `review-spec-implementation` as the post-implementation review gate. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate unbounded Prometheus `query_type` label growth so `/metrics` scraping no longer amplifies memory usage through ever-expanding time series.

**Architecture:** Keep the fix minimal and local to metrics labeling. Introduce one canonical query-type label normalization path with a bounded output set, then route all DNS and DoH metrics through it. Prove the behavior with test-first regression coverage and re-run the existing diagnostics script to validate the memory profile.

**Tech Stack:** Rust 2021, `prometheus` 0.13, `axum` 0.8, `hickory-proto` 0.24, PowerShell diagnostics script, `cargo test`

---

## Chunk 1: Bounded Label Design

### Task 1: Add Regression Test for Bounded Query Type Labels

**Files:**

- Modify: `src/metrics.rs`
- Create: `tests/metrics_label_cardinality_test.rs`
- Reference: `tools/diagnostics/metrics-memory-profile.ps1`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn metrics_query_type_labels_collapse_unknown_record_types_into_other() {
    let metrics = DnsMetrics::new();

    for raw in 1000u16..1100u16 {
        let record_type = RecordType::from(raw);
        let label = normalize_query_type_label(record_type);
        metrics.dns_query_type_total().with_label_values(&[label]).inc();
    }

    let output = metrics.export_metrics();
    assert!(output.contains("type=\"OTHER\""));
    assert_eq!(count_query_type_series(&output), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test metrics_query_type_labels_collapse_unknown_record_types_into_other --test metrics_label_cardinality_test -- --exact`
Expected: FAIL because normalization helper does not exist yet, or because metrics output still contains multiple unknown `query_type` series.

- [ ] **Step 3: Write minimal implementation**

Implementation notes:

```rust
pub fn normalize_query_type_label(record_type: RecordType) -> &'static str {
    match record_type {
        RecordType::A => "A",
        RecordType::AAAA => "AAAA",
        ...
        _ => "OTHER",
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test metrics_query_type_labels_collapse_unknown_record_types_into_other --test metrics_label_cardinality_test -- --exact`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add tests/metrics_label_cardinality_test.rs src/metrics.rs
git commit -m "test: add bounded query type metrics regression"
```

### Task 2: Reuse the Bounded Label Helper Across All Query-Type Metrics

**Files:**

- Modify: `src/doh/handlers.rs`
- Modify: `src/handler.rs`
- Modify: `src/server.rs`
- Modify: `src/metrics.rs`
- Test: `tests/metrics_label_cardinality_test.rs`

- [ ] **Step 1: Extend the failing test to cover real call sites**

```rust
#[test]
fn metrics_recording_paths_use_bounded_query_type_labels() {
    // exercise DNS path + DoH path helpers with uncommon RecordType values
    // export metrics and assert unknown values are emitted as OTHER only
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test metrics_recording_paths_use_bounded_query_type_labels --test metrics_label_cardinality_test -- --exact`
Expected: FAIL because one or more call sites still use `to_string()` or `Cow::Owned`.

- [ ] **Step 3: Write minimal implementation**

Implementation notes:

```rust
let query_type_label = normalize_query_type_label(query_type);
metrics.with_label_values(&[query_type_label, ...])
```

Required replacements:

- `src/handler.rs`: replace direct `query_type.to_string()` labels
- `src/server.rs`: replace direct `query_type.to_string()` labels
- `src/doh/handlers.rs`: replace `record_type_to_cow_str` for metrics labels with bounded helper

- [ ] **Step 4: Run targeted tests to verify they pass**

Run: `cargo test metrics_label_cardinality --test metrics_label_cardinality_test`
Expected: PASS

- [ ] **Step 5: Run impacted suites**

Run: `cargo test --test http_doh_tests --test server_adapter_test`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/metrics.rs src/doh/handlers.rs src/handler.rs src/server.rs tests/metrics_label_cardinality_test.rs
git commit -m "fix: bound prometheus query type label cardinality"
```

## Chunk 2: Diagnostics Verification

### Task 3: Re-run Memory Diagnostics and Compare Profiles

**Files:**

- Reference: `tools/diagnostics/metrics-memory-profile.ps1`
- Reference: `artifacts/metrics-only-profile/process-samples.csv`
- Reference: `artifacts/resolve-only-profile/process-samples.csv`
- Create: `artifacts/metrics-label-fix-profile/`

- [ ] **Step 1: Run the diagnostics script with bounded labels**

Run: `powershell -ExecutionPolicy Bypass -File .\tools\diagnostics\metrics-memory-profile.ps1 -OutputDir .\artifacts\metrics-label-fix-profile -ResolveCount 200 -ScrapeCount 300`
Expected: completes successfully and writes ETL, CSV, stdout, and stderr outputs.

- [ ] **Step 2: Compare process samples**

Run:

```powershell
Get-Content .\artifacts\metrics-label-fix-profile\process-samples.csv
Get-Content .\artifacts\resolve-only-profile\process-samples.csv
Get-Content .\artifacts\metrics-only-profile\process-samples.csv
```

Expected:

- fixed profile should not show continued private-memory growth proportional to distinct query types
- `/metrics` scraping may still raise RSS slightly, but should plateau quickly

- [ ] **Step 3: Run final regression test sweep**

Run: `cargo test`
Expected: PASS

- [ ] **Step 4: Capture implementation summary for handoff**

Record:

- failing test name observed during RED
- minimal production change made during GREEN
- diagnostics delta before/after
- residual risk: allocator high-water RSS may still exist, but unbounded label growth must be gone

- [ ] **Step 5: Commit**

```bash
git add artifacts/metrics-label-fix-profile tests/metrics_label_cardinality_test.rs src/metrics.rs src/doh/handlers.rs src/handler.rs src/server.rs
git commit -m "chore: verify bounded metrics labels with diagnostics"
```

## Acceptance Criteria

- Unknown or uncommon DNS record types no longer create distinct Prometheus `query_type` labels.
- All `query_type`-based metrics use one shared normalization path.
- Regression tests fail before implementation and pass after implementation.
- Existing impacted tests pass.
- Diagnostics show the pre-fix growth mode is eliminated or materially flattened.

## Non-Goals

- Do not redesign the metrics export endpoint.
- Do not treat `mimalloc` tuning as the primary fix.
- Do not remove existing stable, bounded labels like status code or configured upstream group.
