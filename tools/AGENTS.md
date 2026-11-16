# Working in `tools/`

This adds to the root `AGENTS.md`, whose rules all apply here.

All infrastructure schedules the machine's CPU through the token pool in
`src/beryl_tools/tokens.py`: a command that uses many cores takes a lease
(`tokens.acquire`, or `tokens.run_program` for a built program) and sizes its
threads to it, and long background work runs at low priority (`system.utility`)
instead. Cargo commands take tokens through the shim in `shims/cargo`, and the
Lima machines through `bench.vm_ensure`.
