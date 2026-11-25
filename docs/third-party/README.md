# Supplemental dependency notices

`CC0-1.0.txt` is the standard CC0 1.0 Universal text, provided for the
`hexf-parse` dependency, whose published crate declares CC0-1.0 but omits a copy.
It was copied from Ubuntu's `common-licenses` package. The standard text is also
available from [SPDX](https://github.com/spdx/license-list-data/blob/main/text/CC0-1.0.txt).

`clipboard-win-5.4.3-LICENSE.txt` is the Boost Software License 1.0 text from
[the exact upstream revision](https://github.com/DoumanAsh/clipboard-win/blob/ed27008ae5d1f10cfbb266de039f0fb35fd05a56/LICENSE)
recorded in that crate's `.cargo_vcs_info.json`. The published crate omits this file.

The packager includes license and notice files from the locked Cargo dependency
sources and the Rust toolchain. For crates declaring Apache-2.0 that omit the
license text, it includes the standard Apache-2.0 text from the same dependency set.
The package's `THIRD_PARTY.txt` identifies each component and its source.

## Bundled Windows graphics

The Mesa binary archive omits its license texts. `scripts/graphics_notices.py`
collects license files and copyright notices from pinned Mesa 26.3.1, LLVM
23.1.5 and MinGW sources, and adds the related dependency notices. Source URLs,
sizes and SHA-256 hashes are pinned in `scripts/graphics-sources.json`.
These generated notices are embedded in the EXE under `runtime-notices/graphics`.
The original source archives and graphics build scripts are retained in the
release's corresponding runtime-source archive.

