use anyhow::{Context as _, Result, anyhow, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

// ponytail: serialize compiler processes to bound memory; add a small worker pool if needed.
static COMPILER: LazyLock<smol::lock::Semaphore> = LazyLock::new(|| smol::lock::Semaphore::new(1));

pub struct RenderedPage {
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub page: usize,
    pub count: usize,
}

pub async fn compile_document(path: PathBuf, cancelled: Arc<AtomicBool>) -> Result<Vec<u8>> {
    let _permit = COMPILER.acquire().await;
    smol::unblock(move || compile_tex(&path, &cancelled)).await
}

pub async fn render_math(
    latex: String,
    preamble: String,
    package_directory: Option<PathBuf>,
    display: bool,
    font_size: f32,
    color: [u8; 3],
    cancelled: Arc<AtomicBool>,
) -> Result<RenderedPage> {
    let _permit = COMPILER.acquire().await;
    smol::unblock(move || {
        ensure!(!cancelled.load(Ordering::Relaxed), "Rendering cancelled");
        ensure!(
            latex.len() + preamble.len() <= 262144,
            "Equation or preamble is too large"
        );
        let temporary = tempfile::tempdir()?;
        let path = temporary.path().join("equation.tex");
        let document = math_document(&latex, &preamble, display, font_size, color);
        fs::write(&path, document)?;
        let pdf = compile_tex_in(&path, &cancelled, package_directory.as_deref())?;
        let rendered = render_pdf_page(Arc::new(pdf), 0, 1.0)?;
        ensure!(
            rendered.count == 1,
            "An equation must produce exactly one page"
        );
        Ok(rendered)
    })
    .await
}

fn math_document(
    latex: &str,
    preamble: &str,
    display: bool,
    font_size: f32,
    [r, g, b]: [u8; 3],
) -> String {
    let style = if display {
        r"\displaystyle "
    } else {
        r"\textstyle "
    };
    format!(
        "\\documentclass[border=2pt]{{standalone}}\n\\usepackage{{xcolor}}\n{preamble}\n\\begin{{document}}\n\\fontsize{{{font_size}}}{{{font_size}}}\\selectfont\n\\color[RGB]{{{r},{g},{b}}}\n${style}{latex}$\n\\end{{document}}\n"
    )
}

fn compile_tex(path: &Path, cancelled: &AtomicBool) -> Result<Vec<u8>> {
    compile_tex_in(path, cancelled, None)
}

fn compile_tex_in(
    path: &Path,
    cancelled: &AtomicBool,
    directory: Option<&Path>,
) -> Result<Vec<u8>> {
    ensure!(!cancelled.load(Ordering::Relaxed), "Compilation cancelled");
    if let Some(directory) = directory {
        ensure!(
            directory.is_absolute() && directory.is_dir(),
            "latex.package_directory must be an existing absolute directory"
        );
    }
    let output = tempfile::tempdir()?;
    let log_path = output.path().join("compiler-output.txt");
    let log = fs::File::create(&log_path)?;
    let executable = std::env::current_exe()?.with_file_name(if cfg!(windows) {
        "tectonic.exe"
    } else {
        "tectonic"
    });
    let executable = if executable.is_file() {
        executable
    } else {
        PathBuf::from("tectonic")
    };
    let mut command = Command::new(executable);
    command
        .args(["--untrusted", "--keep-logs", "--outdir"])
        .arg(output.path())
        .arg(if directory.is_some() {
            Path::new("-")
        } else {
            path
        })
        .current_dir(
            directory
                .or_else(|| path.parent())
                .context("Document has no parent directory")?,
        )
        .stdin(if directory.is_some() {
            Stdio::from(fs::File::open(path)?)
        } else {
            Stdio::null()
        })
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().context(
        "Could not start Tectonic. Put tectonic.exe beside Zed or install Tectonic on PATH",
    )?;
    let start = Instant::now();
    let status = loop {
        if cancelled.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(180) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("LaTeX compilation cancelled or exceeded three minutes");
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        let log = fs::read_to_string(log_path).unwrap_or_default();
        let tail: String = log
            .chars()
            .rev()
            .take(8000)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        anyhow::bail!("LaTeX compilation failed:\n{tail}");
    }
    let pdf = if directory.is_some() {
        output.path().join("texput.pdf")
    } else {
        output
            .path()
            .join(path.file_name().context("Missing filename")?)
            .with_extension("pdf")
    };
    ensure!(
        fs::metadata(&pdf)?.len() <= 100 * 1024 * 1024,
        "PDF exceeds the 100 MB preview limit"
    );
    fs::read(pdf).context("Compiler did not produce a readable PDF")
}

pub fn render_pdf_page(bytes: Arc<Vec<u8>>, page: usize, zoom: f32) -> Result<RenderedPage> {
    // ponytail: reparse on page/zoom changes; cache the parsed document if navigation becomes slow.
    let pdf =
        hayro::hayro_syntax::Pdf::new(bytes).map_err(|e| anyhow!("Could not read PDF: {e:?}"))?;
    let pages = pdf.pages();
    ensure!(!pages.is_empty(), "PDF contains no pages");
    let page_index = page.min(pages.len() - 1);
    let page = &pages[page_index];
    let (width, height) = page.render_dimensions();
    let scale = 1.5 * zoom;
    ensure!(
        zoom.is_finite()
            && zoom > 0.0
            && width.is_finite()
            && height.is_finite()
            && width > 0.0
            && height > 0.0
            && width * scale <= 8192.0
            && height * scale <= 8192.0,
        "Page is too large to preview at this zoom"
    );
    let pixmap = hayro::render(
        page,
        &Default::default(),
        &hayro::RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: hayro::vello_cpu::color::palette::css::TRANSPARENT,
            ..Default::default()
        },
    );
    let pixels = pixmap
        .data()
        .iter()
        .flat_map(|p| [p.b, p.g, p.r, p.a])
        .collect();
    Ok(RenderedPage {
        bgra: pixels,
        width: pixmap.width() as u32,
        height: pixmap.height() as u32,
        page: page_index,
        count: pages.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires Tectonic on PATH and access to its package bundle"]
    fn packages_render_in_documents_and_equations() -> Result<()> {
        smol::block_on(async {
            let directory = tempfile::tempdir()?;
            fs::write(
                directory.path().join("previewlocal.sty"),
                r"\ProvidesPackage{previewlocal}\newcommand{\localnumber}{\mathbb{R}}",
            )?;
            let preamble = r"\usepackage{amsmath,amssymb,previewlocal}";
            let source = directory.path().join("document.tex");
            fs::write(
                &source,
                format!(
                    "\\documentclass{{article}}\n{preamble}\n\\begin{{document}}$\\localnumber$\\newpage Second page\\end{{document}}"
                ),
            )?;
            let cancelled = Arc::new(AtomicBool::new(false));
            let pdf = Arc::new(compile_document(source, cancelled.clone()).await?);
            let page = render_pdf_page(pdf.clone(), 99, 0.5)?;
            assert_eq!((page.page, page.count), (1, 2));
            assert_eq!(
                page.bgra.len(),
                page.width as usize * page.height as usize * 4
            );
            assert!(render_pdf_page(pdf, 0, 0.0).is_err());
            for display in [false, true] {
                let equation = render_math(
                    r"\frac{1}{2}+\localnumber".into(),
                    preamble.into(),
                    Some(directory.path().to_path_buf()),
                    display,
                    16.0,
                    [210, 100, 50],
                    cancelled.clone(),
                )
                .await?;
                assert_eq!(equation.count, 1);
                assert!(equation.width > 10 && equation.height > 10);
                assert!(
                    equation
                        .bgra
                        .chunks_exact(4)
                        .any(|p| p[3] > 0 && p[2] > p[0])
                );
                assert!(equation.bgra.chunks_exact(4).any(|p| p[3] == 0));
            }
            let invalid = render_math(
                r"\undefinedpreviewcommand".into(),
                preamble.into(),
                Some(directory.path().to_path_buf()),
                true,
                16.0,
                [0, 0, 0],
                cancelled,
            )
            .await;
            assert!(
                invalid
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("LaTeX compilation failed")
            );
            Ok(())
        })
    }
}
