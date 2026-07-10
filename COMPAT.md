# Compatibility

This file is updated by the manual `Compatibility` GitHub Actions workflow.

The workflow is intentionally off by default. Run it from the Actions tab with
`run_compat=true` when an external compatibility sweep should clone pinned
source repositories, build generated Rust crates, run their tests, and publish
the resulting status here.
<!-- compat-results:start -->
| Repo | Ref | Status | Failing tests | Result | Updated | Run |
| --- | --- | --- | ---: | --- | --- | --- |
| [remeda](https://github.com/remeda/remeda.git) | `3c80f28bb394` | `passed` | 0 | test result: ok. 1789 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.76s | 2026-07-10T08:56:29+00:00 | [run](https://github.com/Bombatomica64/smelt/actions/runs/29081249732) |
<!-- compat-results:end -->
