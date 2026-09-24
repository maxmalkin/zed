use std::{
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use collections::{HashMap, HashSet};
use gpui::{AnyElement, Context, Hsla, RenderImage, Rgba, SharedString, Task, div, img, px};
use settings::{RegisterSetting, Settings};
use ui::prelude::*;

use crate::Markdown;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MathKey {
    latex: SharedString,
    display: bool,
    font_size: u32,
    color: [u32; 4],
    preamble: SharedString,
    package_directory: Option<PathBuf>,
}

impl MathKey {
    pub(crate) fn new(
        latex: &str,
        display: bool,
        font_size: f32,
        color: Hsla,
        cx: &gpui::App,
    ) -> Self {
        let settings = LatexSettings::get_global(cx);
        let color = Rgba::from(color);
        Self {
            latex: latex.into(),
            display,
            font_size: font_size.to_bits(),
            color: [color.r, color.g, color.b, color.a].map(f32::to_bits),
            preamble: settings.preamble.clone(),
            package_directory: settings.package_directory.clone(),
        }
    }
}

struct MathImage {
    image: Arc<RenderImage>,
    width: f32,
    height: f32,
}

struct CachedMath {
    result: Arc<OnceLock<anyhow::Result<MathImage>>>,
    batch: Option<Arc<MathBatch>>,
}

#[derive(Default)]
pub(crate) struct MathState(HashMap<MathKey, CachedMath>);

impl MathState {
    pub(crate) fn clear(&mut self, cx: &mut gpui::App) {
        for cached in self.0.values() {
            if let Some(Ok(rendered)) = cached.result.get() {
                cx.drop_image(rendered.image.clone(), None);
            }
        }
        self.0.clear();
    }

    pub(crate) fn render(&mut self, key: MathKey, _cx: &mut Context<Markdown>) -> AnyElement {
        let cached = self.0.entry(key.clone()).or_insert_with(|| {
            let result = Arc::new(OnceLock::new());
            CachedMath {
                result,
                batch: None,
            }
        });
        match cached.result.get() {
            Some(Ok(rendered)) => div()
                .child(
                    img(rendered.image.clone())
                        .w(px(rendered.width))
                        .h(px(rendered.height)),
                )
                .into_any_element(),
            Some(Err(error)) => div()
                .id("latex-error")
                .child(format!("{}", key.latex))
                .tooltip(ui::Tooltip::text(format!("LaTeX: {error:#}")))
                .into_any_element(),
            // Keep the source visible while rendering.
            _ => div()
                .child(if key.display {
                    format!("$${}$$", key.latex)
                } else {
                    format!("${}$", key.latex)
                })
                .into_any_element(),
        }
    }

    pub(crate) fn retain(&mut self, used: &HashSet<MathKey>, cx: &mut Context<Markdown>) {
        self.0.retain(|key, cached| {
            if used.contains(key) {
                return true;
            }
            if let Some(Ok(rendered)) = cached.result.get() {
                cx.drop_image(rendered.image.clone(), None);
            }
            false
        });
        let mut groups: HashMap<(SharedString, Option<PathBuf>), Vec<MathKey>> = HashMap::default();
        for (key, cached) in &self.0 {
            if cached.batch.is_none() {
                groups
                    .entry((key.preamble.clone(), key.package_directory.clone()))
                    .or_default()
                    .push(key.clone());
            }
        }
        for ((preamble, directory), keys) in groups {
            // ponytail: cap batches at 32 equations to bound PDF/raster memory.
            for keys in keys.chunks(32) {
                let outputs: Vec<_> = keys.iter().map(|key| self.0[key].result.clone()).collect();
                let equations = keys
                    .iter()
                    .map(|key| {
                        let [r, g, b, _] = key.color.map(f32::from_bits);
                        latex_renderer::MathEquation {
                            latex: key.latex.to_string(),
                            display: key.display,
                            font_size: f32::from_bits(key.font_size),
                            color: [r, g, b].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8),
                        }
                    })
                    .collect();
                let cancelled = Arc::new(AtomicBool::new(false));
                let cancellation = cancelled.clone();
                let preamble = preamble.to_string();
                let directory = directory.clone();
                let task = cx.spawn(async move |markdown, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(75))
                        .await;
                    let results = latex_renderer::render_math_batch(
                        equations,
                        preamble,
                        directory,
                        cancellation,
                    )
                    .await;
                    for (output, rendered) in outputs.into_iter().zip(results) {
                        let rendered = rendered.and_then(|rendered| {
                            let buffer = image::RgbaImage::from_raw(
                                rendered.width,
                                rendered.height,
                                rendered.bgra,
                            )
                            .ok_or_else(|| anyhow::anyhow!("Invalid equation raster"))?;
                            Ok(MathImage {
                                image: Arc::new(RenderImage::new(smallvec::smallvec![
                                    image::Frame::new(buffer)
                                ])),
                                width: rendered.width as f32 / latex_renderer::MATH_RASTER_SCALE,
                                height: rendered.height as f32 / latex_renderer::MATH_RASTER_SCALE,
                            })
                        });
                        output.set(rendered).ok();
                    }
                    markdown.update(cx, |_, cx| cx.notify()).ok();
                });
                let batch = Arc::new(MathBatch {
                    _task: task,
                    cancelled,
                });
                for key in keys {
                    self.0.get_mut(key).unwrap().batch = Some(batch.clone());
                }
            }
        }
    }
}

struct MathBatch {
    _task: Task<()>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for MathBatch {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone, RegisterSetting)]
struct LatexSettings {
    preamble: SharedString,
    package_directory: Option<PathBuf>,
}

impl Settings for LatexSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let settings = content.latex.clone().unwrap_or_default();
        Self {
            preamble: settings
                .preamble
                .unwrap_or_else(|| r"\usepackage{amsmath,amssymb}".into())
                .into(),
            package_directory: settings.package_directory.map(PathBuf::from),
        }
    }
}
