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
Updates come from new workflow artifacts, not the official updater. Copy any
wanted settings/extensions into the new profile through Zed's normal UI.

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
JavaScript equation language. Large notebooks will compile more slowly than a
KaTeX renderer; compilation is serialized to bound memory.

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
and limits PDF/raster size. This is not a complete sandbox for hostile TeX.

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
