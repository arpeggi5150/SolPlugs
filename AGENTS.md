# SolPlugs — instructions for agents

## CI workflows
- Exception (owner-approved 2026-11-17): `publish.yml` runs its own jobs. It builds the Windows VST3 and CLAP bundles and publishes them to arpeggi5150/SolPlugs-Releases on a `v*` tag, because the shared release workflow needs signing secrets this repo does not have. Keep it narrow: it is not a model for other workflows.
