# Supplemental dependency notices

`CC0-1.0.txt` is the standard CC0 1.0 Universal text, provided for the
`hexf-parse` dependency, whose published crate declares CC0-1.0 but omits a copy.
It was copied from Ubuntu's `common-licenses` package. The standard text is also
available from [SPDX](https://github.com/spdx/license-list-data/blob/main/text/CC0-1.0.txt).

`deno-dependency-notices.json` supplements license files omitted from Deno's
locked Cargo archives. Each entry records its source and SHA-256. Exact package
revisions are used where they contain a license; later upstream license files
are identified explicitly when the older revision omits one.

`deno-v8-notices.json` preserves Rusty V8 150.4.0 and its pinned submodules'
notices, including V8, ICU and the LLVM runtime libraries. Rusty V8's published
crate excludes these files. The Windows builder verifies these notice hashes,
collects all locked Deno crate notices, and includes the source archives in the
separate runtime source download.

The published `deno_core_icudata`, `deno_native_certs` and `fqdn` sources declare
MIT but omit a license and copyright file. Their notices reproduce the standard
MIT permission terms and Cargo author metadata. ICU's data notices are also
included with V8.

`clipboard-win-5.4.3-LICENSE.txt` is the Boost Software License 1.0 text from
[the exact upstream revision](https://github.com/DoumanAsh/clipboard-win/blob/ed27008ae5d1f10cfbb266de039f0fb35fd05a56/LICENSE)
recorded in that crate's `.cargo_vcs_info.json`. The published crate omits this file.

`harfrust-0.5.2-LICENSE.txt` preserves the MIT permission and copyright notices
from [its exact upstream revision](https://github.com/harfbuzz/harfrust/blob/efdae3142ab76a2f1524d72cff9e3dfdc5afd7ca/LICENSE),
recorded in the crate's `.cargo_vcs_info.json`. The published crate omits the
repository-root license file.

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

