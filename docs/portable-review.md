# Portable checkpoint review

Fixed review range: `a4d37cf...d074de6`. Two independent read-only reviews used the requirements in [portable 0.0.1 spec](portable-0.0.1-spec.md) and the documented [contribution standards](../CONTRIBUTING.md).

## Standards findings

- Combined `--ssh --help` extracted the runtime and showed the thin-build runtime requirement. Reproduced on the standalone Windows executable. The launcher now recognizes help/version among options while respecting option values and the `--` delimiter; unit and executable regressions cover this.
- The supervisor did not own its child's lifetime or handle termination. Unix now forwards termination and waits, both binaries return through cleanup guards, and Windows uses a kill-on-close Job Object. Unix executable verification includes an actual PID-targeted SIGTERM test requiring child exit, terminal restoration and extraction cleanup. Uncatchable process kills can still leave temporary files.

## Specification findings

- Both Windows executables imported `VCRUNTIME140.dll`, preventing startup on a clean PC without that redistributable. PE inspection reproduced this independently of the architecture mismatch. Both Windows targets now enable static CRT linkage, with an import-table gate on the outer and extracted executable.
- A source-recipe superset was treated as sufficient corresponding-source evidence. Build manifests now distinguish verified upstream archives, unverified local runtimes and source correspondence. Publication rejects unresolved correspondence. Exact Windows/macOS binary-to-contrib-source mapping remains an open release blocker; it is not described as a completed certification.

## Additional runtime and queue checks

Playlist paths resolve against the playlist file, exports are atomic, and duplicate entries advance by occurrence. Disabling shuffle restores both the original order and the current occurrence. See [validation](../VALIDATION.md) for completed execution evidence and remaining acceptance work.
