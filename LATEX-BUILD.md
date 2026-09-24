# Zed LaTeX personal build

This fork adds native TeX previews to Zed. A normal Zed extension cannot register
these preview views; the changes live in the editor source. The extensions fork
is kept current but needs no changes for rendering.

## Windows installation

Run the **Custom Windows Zed** workflow on branch `latex-preview`. Download the
`Zed-LaTeX-Windows-x64-with-WSL-SSH` artifact, extract the entire archive, and run
`zed-latex.exe`. Keep all bundled files together: `cli.exe` also provides the
SSH password helper, and `tectonic.exe` provides compilation.

This is a portable, unsigned development build. It uses separate `Zed-LaTeX`
configuration and data directories and does not replace the official install.
Updates come from custom GitHub Releases, not the official updater. Official Zed configuration is inherited read-only; custom settings override it.

The workflow builds a matching Linux x86_64 remote server for WSL and Linux SSH
hosts. Other remote platforms require `remote_server` built from the exact same
commit and gzipped at `remote_servers/<os>-<arch>/remote_server.gz` beside the app.
Remote server installations use `.zed_latex_server`, separate from official Zed.

## Package imports in Markdown and Jupyter

Put imports and shared macros in the new profile's settings:

```json
{
  "latex": {
    "preamble": "\\usepackage{amsmath,amssymb,mathtools}\n\\newcommand{\\RR}{\\mathbb{R}}",
    "package_directory": "C:\\Users\\Max\\Documents\\tex-packages"
  }
}
```

`package_directory` is optional. If set, it must be an existing absolute path on
the Windows machine. Files such as `mypackage.sty` in that directory can be
imported with `\usepackage{mypackage}` in the preamble. The preamble replaces the
default `\usepackage{amsmath,amssymb}`, so keep those imports if you need them.
Each equation gets the same preamble; definitions in one equation do not carry
to another. Imports belong in the preamble, not inside `$...$`.

Markdown previews and Jupyter Markdown cells support `$...$` and `$$...$$`.
Kernel outputs with MIME type `text/latex` use the same renderer and package
settings. For example, in a Python kernel:

```python
from IPython.display import Latex, display
display(Latex(r"\frac{1}{2}+\RR"))
```

Equations compile asynchronously and show source while loading. Hover a failed
equation to see its compiler error. The first run
needs internet access for Tectonic's TeX package/font bundle; subsequent runs
reuse its cache. Markdown math is rendered through real TeX, not a restricted
JavaScript equation language. Equations sharing a preamble compile in batches of up to 32, with cached images
reused until source or appearance changes. Compilation is serialized to bound
memory. A failed batch retries equations separately so one invalid equation does
not hide its neighbors. Real TeX still has startup costs, especially on first use.

## TeX documents

Open a saved `.tex` document in a trusted local project and run **latex: open
preview** from the command palette. The preview opens to the right, recompiles
when that document is saved, and has Rebuild, page navigation, and zoom controls.
Use the document's normal `\usepackage`, `\input`, and `\include` commands;
local package files are resolved relative to that document's directory. The
Markdown/Jupyter preamble settings do not modify `.tex` documents.

The first version compiles local documents only. SSH/WSL editing remains
available, but compiling a `.tex` document on the remote host is not implemented.
Use Rebuild after changing an included file. There is no SyncTeX navigation or
PDF text selection yet. Examples are included in `examples/`.

Tectonic uses XeTeX and its package bundle. Packages requiring another engine or
external shell commands (for example shell-escape-based minted workflows) are
not supported. Compilation disables shell escape, times out after three minutes,
and limits PDF/raster size (100 MB PDF, 64 MiB document page, 4 MiB equation image). This is not a complete sandbox for hostile TeX.

## Multiplayer removal

The application does not initialize calls, channels, contacts notifications, or
the collaboration panel. Collaboration shortcuts, settings sections, titlebar
controls, and follow-message subscriptions are removed. The WebRTC/LiveKit
network transport and WebRTC audio echo processor are excluded from the build.
Shared Rust types and upstream collaboration test code remain to avoid rewriting
the workspace/project model. SSH/WSL transport remains independent and enabled.

## Validation and development

The standalone renderer test exercises actual compilation and PDF rasterization
with standard packages and a generated local `.sty` file. With Tectonic on PATH:

```sh
cargo test -p latex_renderer -- --ignored
cargo test -p markdown --lib math_
cargo test -p repl --lib latex_outputs_
cargo check -p zed -p repl -p markdown_preview
```

For a native Windows build, install Zed's documented Windows build dependencies,
then run `cargo build --locked --profile release-fast -p zed --bin zed-latex`
from an MSVC developer shell with `ZED_RELEASE_CHANNEL=dev` and
`ZED_UPDATE_EXPLANATION=Update from your fork`. Copy Tectonic beside the result.

Relevant upstream work: [math rendering PR](https://github.com/zed-industries/zed/pull/61892),
[PDF viewer proposal](https://github.com/zed-industries/zed/pull/51870),
[extension capabilities](https://zed.dev/docs/extensions/developing-extensions),
and [Windows build instructions](https://zed.dev/docs/development/windows).

## Keeping the fork current

The **Sync Zed stable** workflow checks daily for the latest official stable
release. It merges that release into a separate branch, opens a PR against
`latex-preview`, and dispatches the native build/tests. Review the PR and its
Actions run before merging. Conflicts stop the workflow and list affected files;
they are never resolved by discarding the custom changes. The initial fork was
based on upstream main, so it retains that snapshot's ahead-of-stable changes
until stable catches up.

Successful builds of `latex-preview` are published by **Publish custom Zed** as
`latex-<run-id>` releases with a complete Windows/remote-server bundle and SHA-256
checksum. Candidate update branches are tested but never published. Scheduled
and workflow-completion triggers require these automation workflows on the
repository's default branch; the build and custom source stay on `latex-preview`.

Close the custom editor, then run its bundled script from PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\Zed-LaTeX\Update-Zed-LaTeX.ps1"
```

Add `-CheckOnly` to check without installing. The script uses only this fork's
latest published release, checks the archive checksum and required files, stages
it before replacement, and keeps one previous bundle in `Zed-LaTeX.previous`.
It does not touch official Zed or either application's settings. To roll back,
close the custom editor and rename the current and previous bundle folders.
Updates are explicitly run; no background scheduled task is installed.

## Official configuration inheritance

Without `--user-data-dir`, Zed LaTeX reads the official Zed configuration as a
base layer and watches it for changes. On Windows this is `%APPDATA%\Zed`;
its own overrides remain in `%APPDATA%\Zed-LaTeX`. Settings (including nested
language settings and profiles) merge with custom values taking precedence.
Keybindings are loaded official-first, custom-last. Tasks/debug arrays are
inherited when absent and replaced by an explicit custom array. Custom themes
load after official user themes; snippet directories are both watched.

The official files are never written by inheritance. Editing settings through
Zed LaTeX edits its own overrides. `--user-data-dir` remains an isolated profile.
Extensions have separate installation directories; missing extensions were
imported from the official installation on this machine, without replacing
existing custom extensions. Future extension installations remain independent.

## Performance checks

Run `cargo test -p latex_renderer -- --include-ignored --test-threads=1 --nocapture`
with Tectonic on PATH. This includes actual package rendering, multi-page equation
batching, bad-equation isolation, alpha conversion, and an eight-equation timing
comparison. Raster images use straight alpha for GPUI and equations use 3 raster
pixels per logical pixel; this corrects faint edges without changing math style.
PDF previews retain the parsed document, coalesce page/zoom requests, and release
GPU images when closed. Compiler output is bounded and only its error tail is
read into memory. No unbounded global equation cache or extra compiler pool is
introduced.

Measured on this machine with a warm Tectonic cache (eight equations):

| Check | Separate compilations | Batched |
| --- | --- | --- |
| Native Windows TeX compilation | 4.717 s | 0.595 s |
| Linux compiler plus rasterizer, debug test build | 4.313 s | 0.917 s |

The eight high-resolution images occupy 864,800 bytes before GPU upload. A
separate `/usr/bin/time -v` run of the renderer benchmark reported maximum RSS
188,416 KiB; this is a compiler/renderer test measurement, not total editor or
GPU memory. Timings depend on packages, document complexity, and hardware.

The review also covered Jupyter MIME routing, multiplayer removal, CLI/remote
bundle selection, and the release updater. Those additions introduce no new
continuous rendering or polling loops; no speculative rewrite was made there.
The updated Windows GUI's sustained memory usage still needs measurement after
its native build completes.
