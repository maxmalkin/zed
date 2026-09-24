# Zed LaTeX

A personal Windows build of [Zed](https://github.com/zed-industries/zed) with real LaTeX previews, inherited editor settings, and human multiplayer features removed. SSH and WSL development remain available.

[Download the latest build](https://github.com/maxmalkin/zed/releases/latest) · [Build status](https://github.com/maxmalkin/zed/actions/workflows/custom-windows.yml?query=branch%3Alatex-preview) · [Detailed guide](https://github.com/maxmalkin/zed/blob/latex-preview/LATEX-BUILD.md)

## What changes

| Area | This fork |
| --- | --- |
| Markdown | Inline `$...$` and display `$$...$$` math in the native preview. |
| Jupyter | Math in Markdown cells and kernel `text/latex` output, using the same renderer. |
| TeX documents | A split preview with rebuild-on-save, page navigation, and zoom. |
| Packages | Real TeX package imports and local `.sty` files on all three surfaces. |
| Appearance | Correct alpha blending, high-resolution equations, and configurable TeX styling. |
| Configuration | Read-only inheritance from official Zed; custom settings and keybindings take precedence. |
| Multiplayer | Calls, channels, contacts UI, project sharing, and following are disabled; WebRTC transport is excluded. |
| Remote development | A matching Linux x86_64 server is bundled for WSL and Linux SSH hosts. |

Rendering uses **Tectonic** for TeX compilation and **Hayro** for PDF rasterization. Equations sharing a preamble compile in batches; unchanged results are cached. This requires a custom editor build because Zed's extension API cannot register these native preview views.

## Install on Windows

1. Download `Zed-LaTeX-Windows-x64.zip` from the [latest release](https://github.com/maxmalkin/zed/releases/latest).
2. Extract the entire archive into `%LOCALAPPDATA%\Programs\Zed-LaTeX`.
3. Run `zed-latex.exe`. Keep `cli.exe`, `tectonic.exe`, the DLLs, and `remote_servers` alongside it.

This is an unsigned personal build. It uses separate configuration and data directories and can run alongside official Zed. The first equation render downloads Tectonic's package/font bundle; later renders reuse that cache.

The implementation lives on **[`latex-preview`](https://github.com/maxmalkin/zed/tree/latex-preview)**. The default `main` branch hosts the repository overview and scheduled automation.

## Use the previews

For Markdown, open a `.md` file and press **Ctrl+Shift+V**, or **Ctrl+K V** for a preview to the side. For a saved `.tex` file in a trusted local project, run **latex: open preview** from the command palette.

Markdown and Jupyter share a preamble in the custom profile's settings. For example:

```json
{
  "latex": {
    "preamble": "\\usepackage{amsmath,amssymb,mathtools}\n\\newcommand{\\RR}{\\mathbb{R}}\n\\boldmath"
  }
}
```

`\boldmath` gives math heavier strokes; omit it for regular TeX weight. The preamble replaces the default imports, so retain `amsmath,amssymb` if needed. To import your own `.sty` files, also set `latex.package_directory` to an **existing absolute Windows directory** and add the corresponding `\usepackage` to the preamble.

TeX documents use their own preambles and resolve local packages relative to the document. The `examples` folder in each release includes Markdown, notebook, TeX, and local-package examples.

## Keep your settings

Official Windows settings are read from `%APPDATA%\Zed`. Overrides are written to `%APPDATA%\Zed-LaTeX`; the official files are left untouched. Settings and keybindings reload when changed. User theme and snippet directories are inherited too. An explicit `--user-data-dir` creates an isolated profile.

Extensions retain separate installation directories. Install any theme or language extensions required by your configuration in the custom profile as well.

## Update without losing the custom features

Close Zed LaTeX, then run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\Zed-LaTeX\Update-Zed-LaTeX.ps1"
```

Add `-CheckOnly` to check without installing. The updater downloads this fork's release, verifies its checksum, and replaces the complete application/server bundle. One previous version is retained in `Zed-LaTeX.previous` for rollback.

A daily workflow checks official stable releases and prepares integration PRs. Conflicts stop for manual resolution. Successful builds of the custom branch publish new releases; candidate branches do not. Updates are explicitly installed with the script above, and do not use official Zed binaries.

## Current limits

- `.tex` compilation currently supports **local projects**. SSH/WSL editing works, but remote-host TeX compilation is not implemented.
- Tectonic uses XeTeX and its package bundle. Packages requiring another engine or external shell commands are not supported.
- Included-file changes require **Rebuild** in the `.tex` preview. SyncTeX and PDF text selection are not implemented.
- Real TeX has startup costs. First-use downloads and errors requiring individual equation retries take longer than warm batched renders.

See the [build and validation guide](https://github.com/maxmalkin/zed/blob/latex-preview/LATEX-BUILD.md) for configuration details, measured performance, resource limits, and regression tests.

## Source and credits

```sh
git clone --branch latex-preview https://github.com/maxmalkin/zed.git
```

Zed is developed by Zed Industries and its contributors. This personal fork is not an official Zed release. The [original upstream README](README.upstream.md) is preserved for reference.

Source licensing remains primarily [GPL-3.0-or-later](LICENSE-GPL), with [Apache-2.0](LICENSE-APACHE) components where marked. See individual component notices for details.
