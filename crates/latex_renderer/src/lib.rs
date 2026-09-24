use anyhow::{Context as _, Result, anyhow, ensure};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
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

pub use hayro::hayro_syntax::Pdf as PdfDocument;

pub struct RenderedPage {
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub page: usize,
    pub count: usize,
}

pub async fn compile_document(path: PathBuf, cancelled: Arc<AtomicBool>) -> Result<PdfDocument> {
    ensure!(!cancelled.load(Ordering::Relaxed), "Rendering cancelled");
    let _permit = COMPILER.acquire().await;
    smol::unblock(move || {
        let bytes = compile_tex(&path, &cancelled)?;
        PdfDocument::new(Arc::new(bytes)).map_err(|e| anyhow!("Could not read PDF: {e:?}"))
    })
    .await
}

/// Raster pixels per logical pixel in Markdown/Jupyter equations.
pub const MATH_RASTER_SCALE: f32 = 3.0;

#[derive(Clone)]
pub struct MathEquation {
    pub latex: String,
    pub display: bool,
    pub font_size: f32,
    pub color: [u8; 3],
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
    let equation = MathEquation {
        latex,
        display,
        font_size,
        color,
    };
    Ok(
        compile_math(vec![equation], preamble, package_directory, cancelled)
            .await?
            .remove(0),
    )
}

/// Compile equations sharing a preamble together, keeping one bad equation from
/// preventing its neighbors from rendering.
pub async fn render_math_batch(
    equations: Vec<MathEquation>,
    preamble: String,
    package_directory: Option<PathBuf>,
    cancelled: Arc<AtomicBool>,
) -> Vec<Result<RenderedPage>> {
    if equations.is_empty() {
        return Vec::new();
    }
    match compile_math(
        equations.clone(),
        preamble.clone(),
        package_directory.clone(),
        cancelled.clone(),
    )
    .await
    {
        Ok(pages) => pages.into_iter().map(Ok).collect(),
        Err(error) if equations.len() == 1 || cancelled.load(Ordering::Relaxed) => equations
            .iter()
            .map(|_| Err(anyhow!("{error:#}")))
            .collect(),
        Err(_) => {
            let mut results = Vec::with_capacity(equations.len());
            for equation in equations {
                results.push(
                    render_math(
                        equation.latex,
                        preamble.clone(),
                        package_directory.clone(),
                        equation.display,
                        equation.font_size,
                        equation.color,
                        cancelled.clone(),
                    )
                    .await,
                );
            }
            results
        }
    }
}

async fn compile_math(
    equations: Vec<MathEquation>,
    preamble: String,
    package_directory: Option<PathBuf>,
    cancelled: Arc<AtomicBool>,
) -> Result<Vec<RenderedPage>> {
    ensure!(!cancelled.load(Ordering::Relaxed), "Rendering cancelled");
    let _permit = COMPILER.acquire().await;
    smol::unblock(move || {
        ensure!(!cancelled.load(Ordering::Relaxed), "Rendering cancelled");
        ensure!(
            preamble.len() + equations.iter().map(|e| e.latex.len()).sum::<usize>() <= 262144,
            "Equations or preamble are too large"
        );
        let temporary = tempfile::tempdir()?;
        let path = temporary.path().join("equation.tex");
        fs::write(&path, math_document(&equations, &preamble))?;
        let bytes = compile_tex_in(&path, &cancelled, package_directory.as_deref())?;
        let pdf = hayro::hayro_syntax::Pdf::new(Arc::new(bytes))
            .map_err(|e| anyhow!("Could not read PDF: {e:?}"))?;
        ensure!(
            pdf.pages().len() == equations.len(),
            "Each equation must produce exactly one page"
        );
        (0..equations.len())
            .map(|page| {
                ensure!(!cancelled.load(Ordering::Relaxed), "Rendering cancelled");
                raster_page(&pdf, page, MATH_RASTER_SCALE / 1.5, 4 * 1024 * 1024)
            })
            .collect()
    })
    .await
}

fn math_document(equations: &[MathEquation], preamble: &str) -> String {
    let mut document = format!(
        "\\documentclass[border=2pt,multi=preview]{{standalone}}\n\\usepackage{{xcolor}}\n{preamble}\n\\begin{{document}}\n"
    );
    for equation in equations {
        let style = if equation.display {
            r"\displaystyle "
        } else {
            r"\textstyle "
        };
        let MathEquation {
            latex,
            font_size,
            color: [r, g, b],
            ..
        } = equation;
        document.push_str(&format!(
            "\\begin{{preview}}\\begingroup\n\\fontsize{{{font_size}}}{{{font_size}}}\\selectfont\n\\color[RGB]{{{r},{g},{b}}}\n${style}{latex}$\n\\endgroup\\end{{preview}}\n"
        ));
    }
    document.push_str("\\end{document}\n");
    document
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
        .args(["--untrusted", "--outdir"])
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
        if fs::metadata(&log_path).is_ok_and(|metadata| metadata.len() > 8 * 1024 * 1024) {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("LaTeX compiler output exceeded 8 MB");
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("Could not wait for LaTeX compiler");
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        let mut log = fs::File::open(log_path)?;
        let offset = log.metadata()?.len().saturating_sub(8000);
        log.seek(SeekFrom::Start(offset))?;
        let mut tail = Vec::new();
        log.read_to_end(&mut tail)?;
        let tail = String::from_utf8_lossy(&tail);
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

pub fn render_pdf_page(pdf: Arc<PdfDocument>, page: usize, zoom: f32) -> Result<RenderedPage> {
    raster_page(&pdf, page, zoom, 64 * 1024 * 1024)
}

fn raster_page(
    pdf: &hayro::hayro_syntax::Pdf,
    page: usize,
    zoom: f32,
    max_bytes: usize,
) -> Result<RenderedPage> {
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
            && height * scale <= 8192.0
            && (width * scale).ceil() * (height * scale).ceil() * 4.0 <= max_bytes as f32,
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
        .flat_map(|p| straight_bgra([p.r, p.g, p.b, p.a]))
        .collect();
    Ok(RenderedPage {
        bgra: pixels,
        width: pixmap.width() as u32,
        height: pixmap.height() as u32,
        page: page_index,
        count: pages.len(),
    })
}

// Hayro emits premultiplied RGBA; GPUI's image shader expects straight BGRA.
// Feeding premultiplied colors to it applies alpha twice and thins glyph edges.
fn straight_bgra([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    let straight = |c: u8| {
        if a == 0 {
            0
        } else {
            ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
        }
    };
    [straight(b), straight(g), straight(r), a]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires Tectonic; reports warm-cache rendering time"]
    fn measure_equation_batch() -> Result<()> {
        smol::block_on(async {
            let equations: Vec<_> = (0..8)
                .map(|i| MathEquation {
                    latex: format!(r"\frac{{{i}+1}}{{2}}+\mathbb{{R}}"),
                    display: true,
                    font_size: 16.0,
                    color: [230; 3],
                })
                .collect();
            let preamble = r"\usepackage{amsmath,amssymb}";
            let cancelled = Arc::new(AtomicBool::new(false));
            compile_math(
                vec![equations[0].clone()],
                preamble.into(),
                None,
                cancelled.clone(),
            )
            .await?;
            let start = Instant::now();
            let mut sizes = Vec::new();
            for equation in &equations {
                let page = render_math(
                    equation.latex.clone(),
                    preamble.into(),
                    None,
                    equation.display,
                    equation.font_size,
                    equation.color,
                    cancelled.clone(),
                )
                .await?;
                sizes.push((page.width, page.height));
            }
            let serial = start.elapsed();
            let start = Instant::now();
            let pages = compile_math(equations, preamble.into(), None, cancelled).await?;
            let batch = start.elapsed();
            assert_eq!(
                pages
                    .iter()
                    .map(|page| (page.width, page.height))
                    .collect::<Vec<_>>(),
                sizes
            );
            eprintln!(
                "8 equations: separate={serial:?}, batch={batch:?}, cached raster bytes={}",
                pages.iter().map(|page| page.bgra.len()).sum::<usize>()
            );
            Ok(())
        })
    }

    #[test]
    fn cancelled_documents_do_not_start_a_compiler() {
        let result = smol::block_on(compile_document(
            PathBuf::from("nonexistent.tex"),
            Arc::new(AtomicBool::new(true)),
        ));
        assert!(result.err().unwrap().to_string().contains("cancelled"));
    }

    #[test]
    fn raster_colors_apply_alpha_once() {
        assert_eq!(straight_bgra([64, 32, 16, 128]), [32, 64, 128, 128]);
        assert_eq!(straight_bgra([255, 255, 255, 255]), [255, 255, 255, 255]);
        assert_eq!(straight_bgra([0, 0, 0, 0]), [0, 0, 0, 0]);
    }

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
            assert!(raster_page(&pdf, 0, 0.5, 1).is_err());
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
            let equations = vec![
                MathEquation {
                    latex: r"\localnumber".into(),
                    display: false,
                    font_size: 16.0,
                    color: [255; 3],
                },
                MathEquation {
                    latex: r"\frac{1}{2}+\localnumber".into(),
                    display: true,
                    font_size: 16.0,
                    color: [255; 3],
                },
            ];
            let batch = compile_math(
                equations.clone(),
                preamble.into(),
                Some(directory.path().to_path_buf()),
                cancelled.clone(),
            )
            .await?;
            assert_eq!(batch.len(), 2);
            assert_eq!(batch[0].count, 2);
            assert!(batch[1].height > batch[0].height);
            assert!(
                batch[0]
                    .bgra
                    .chunks_exact(4)
                    .any(|p| p[3] > 30 && p[3] < 220 && p[0] > 245)
            );
            let mut mixed = equations;
            mixed[1].latex = r"\undefinedpreviewcommand".into();
            let results = render_math_batch(
                mixed,
                preamble.into(),
                Some(directory.path().to_path_buf()),
                cancelled.clone(),
            )
            .await;
            assert!(results[0].is_ok());
            assert!(results[1].is_err());
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
