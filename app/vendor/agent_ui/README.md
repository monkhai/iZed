# Agent UI override

This is Zed's `crates/agent_ui` from commit
`5688167d224b5eca54875d49afb8bfd73a07915a`, the revision used by iZed.
It is licensed under GPL-3.0-or-later; see `LICENSE-GPL`.

iZed changes `src/agent_panel.rs` so Agent text zoom leaves the panel toolbar
at the app's UI size. The dependency manifest resolves the rest of Zed's
crates from that same commit. When updating Zed, compare this directory with
the new `crates/agent_ui` before changing the revision.
