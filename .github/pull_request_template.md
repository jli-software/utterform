## Summary

<!-- What changes for the user, and why? -->

## Validation

<!-- List the exact checks run and any native/manual coverage still outstanding. -->

- [ ] Svelte/TypeScript checks and unit tests
- [ ] Rust formatting, Clippy and native tests
- [ ] Production browser scenarios
- [ ] Relevant platform-specific checks, or not applicable with reason

## Diagnostics and privacy

- [ ] New or changed failure, fallback, external-I/O and durable-state paths have actionable,
      correlated diagnostic events, or this PR explains why diagnostics are not applicable.
- [ ] Repeated or latency-sensitive paths are aggregated/rate-limited; no logging occurs in audio,
      input or rendering hot loops.
- [ ] Logs contain no credentials, transcript/history/clipboard/file/audio content, prompts,
      vocabulary, recording context, window titles or raw request/response bodies.
- [ ] Diagnostic formatting, error/fallback coverage, redaction and output bounds have tests where
      this PR changes them.

## Release boundary

- [ ] This PR does not publish, deploy, tag or move an existing release.
