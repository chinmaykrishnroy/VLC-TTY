# Supplemental dependency notices

Some published Cargo packages omit their license texts. `notices/` retains those texts from the exact repository revisions recorded by their `.cargo_vcs_info.json` files. `notice-sources.json` records immutable source URLs and SHA256 digests.

The packager copies these alongside license texts provided by other Cargo packages and checks their hashes. A new dependency version without a packaged license or a matching supplement stops packaging. Update notices by inspecting that version's source and preserving applicable copyright notices; do not substitute a generic SPDX template.

The archive's `THIRD_PARTY.json` is the target-filtered Cargo package inventory. It can include resolved optional dependencies; it is not a claim that every listed crate is linked. LibVLC is installed separately and is not included in these archives.
