# Publishing compiler progress and the website

Publish every verified batch to **https://github.com/benfavre/ts-rs**, branch
`main`, before updating https://ts-rs.bext.dev/. A push to another repository
does not satisfy this publication step.

The public checkout may have a separate publication history. Apply verified
compiler commits onto its existing `main`; preserve its licensing, packaging,
and documentation changes. Do not replace public history with an internal
checkout's history. Validate the public checkout, since workspace membership
and Rust test counts can differ even when compiler behavior is identical.

The current public source lives under `website/`. Its metrics come from
`docs/compatibility-metrics.json`; reproduction commands and report hashes are
stored alongside each count. Update that snapshot from cache-free reports and
refresh the README before publishing. Retain the measurement commit rather
than attributing old results to a later documentation commit. Keep historical
snapshots explicitly dated.

After the code, snapshot, and website source are committed:

```bash
git push origin main
cd website
bun scripts/sync.ts ..
```

The sync requires the checkout's HEAD to equal public `main`, reads inputs
with `git show`, validates the snapshot and both diagnostic wave totals, and
only then writes generated content. It also expands documentation metrics
for the pages and search index using the same snapshot. An unpublished
checkout is rejected without changing outputs.

Copy the reviewed source and generated outputs to the configured live PRISM
site. Preserve its existing WASM package unless rebuilding it separately.
The normal source watcher reloads the site. Verify the homepage, conformance,
progress, affected docs, and search over HTTPS, including the public commit
in `data-build`; inspect desktop/mobile rendering and browser errors.
Deployment is complete only when those live checks pass. See
[website/README.md](../website/README.md) for runtime and asset details.
