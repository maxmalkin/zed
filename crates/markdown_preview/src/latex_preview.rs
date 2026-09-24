use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use editor::Editor;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, Render, RenderImage, Subscription,
    Task, Window, actions, img,
};
use language::{Buffer, BufferEvent};
use project::{Project, trusted_worktrees::TrustedWorktrees};
use ui::prelude::*;
use workspace::{Workspace, item::Item};

actions!(
    latex,
    [
        /// Compiles the saved TeX document and opens its preview to the side.
        OpenPreview,
    ]
);

pub(super) fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &OpenPreview, window, cx| {
            let Some(editor) = workspace.active_item_as::<Editor>(cx) else {
                return;
            };
            let Some(buffer) = editor.read(cx).buffer().read(cx).as_singleton() else {
                return;
            };
            let Some(file) = buffer.read(cx).file() else {
                return;
            };
            if file
                .path()
                .as_std_path()
                .extension()
                .and_then(|e| e.to_str())
                != Some("tex")
            {
                return;
            }
            let path = file
                .as_local()
                .map(|file| file.abs_path(cx))
                .unwrap_or_else(|| file.full_path(cx));
            let project = workspace.project().clone();
            let view = cx.new(|cx| LatexPreview::new(path, buffer, project, cx));
            let pane = workspace.split_pane(
                workspace.active_pane().clone(),
                workspace::SplitDirection::Right,
                window,
                cx,
            );
            workspace.add_item(pane, Box::new(view), None, true, true, window, cx);
        });
    })
    .detach();
}

struct LatexPreview {
    path: PathBuf,
    buffer: Entity<Buffer>,
    project: Entity<Project>,
    focus: FocusHandle,
    pdf: Option<Arc<latex_renderer::PdfDocument>>,
    image: Option<Arc<RenderImage>>,
    page: usize,
    page_count: usize,
    zoom: f32,
    message: SharedString,
    compiling: bool,
    rendering: bool,
    render_again: bool,
    compile_again: bool,
    cancelled: Arc<AtomicBool>,
    _compile: Task<()>,
    _render: Task<()>,
    _subscription: Subscription,
}

impl LatexPreview {
    fn new(
        path: PathBuf,
        buffer: Entity<Buffer>,
        project: Entity<Project>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_release(|this, cx| {
            if let Some(image) = this.image.take() {
                cx.drop_image(image, None);
            }
        })
        .detach();
        let subscription = cx.subscribe(&buffer, |this, _, event, cx| {
            if matches!(event, BufferEvent::Saved) {
                this.compile(cx);
            }
        });
        let mut this = Self {
            path,
            buffer,
            project,
            focus: cx.focus_handle(),
            pdf: None,
            image: None,
            page: 0,
            page_count: 0,
            zoom: 1.0,
            message: "".into(),
            compiling: false,
            rendering: false,
            render_again: false,
            compile_again: false,
            cancelled: Arc::new(AtomicBool::new(false)),
            _compile: Task::ready(()),
            _render: Task::ready(()),
            _subscription: subscription,
        };
        this.compile(cx);
        this
    }

    fn compile(&mut self, cx: &mut Context<Self>) {
        if self.compiling {
            self.compile_again = true;
            return;
        }
        let project = self.project.read(cx);
        if project.is_remote() {
            self.message = "LaTeX compilation currently requires a local project.".into();
        } else if TrustedWorktrees::has_restricted_worktrees(&project.worktree_store(), cx) {
            self.message = "Trust this project before compiling LaTeX.".into();
        } else if self.buffer.read(cx).is_dirty() {
            self.message = "Save the document to compile its preview.".into();
        } else {
            self.path = self
                .buffer
                .read(cx)
                .file()
                .and_then(|f| f.as_local().map(|f| f.abs_path(cx)))
                .unwrap_or(self.path.clone());
            self.compiling = true;
            self.message = "Compiling LaTeX…".into();
            let path = self.path.clone();
            let cancelled = self.cancelled.clone();
            let task = cx.background_spawn(async move {
                latex_renderer::compile_document(path, cancelled).await
            });
            self._compile = cx.spawn(async move |this, cx| {
                let result = task.await;
                this.update(cx, |this, cx| {
                    this.compiling = false;
                    match result {
                        Ok(pdf) => {
                            this.pdf = Some(Arc::new(pdf));
                            this.message = "".into();
                            this.render_page(cx);
                        }
                        Err(error) => this.message = format!("{error:#}").into(),
                    }
                    if std::mem::take(&mut this.compile_again) {
                        this.compile(cx);
                    }
                    cx.notify();
                })
                .ok();
            });
        }
        cx.notify();
    }

    fn render_page(&mut self, cx: &mut Context<Self>) {
        if self.rendering {
            self.render_again = true;
            return;
        }
        let Some(pdf) = self.pdf.clone() else { return };
        self.rendering = true;
        let page = self.page;
        let zoom = self.zoom;
        let task =
            cx.background_spawn(async move { latex_renderer::render_pdf_page(pdf, page, zoom) });
        self._render = cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.rendering = false;
                if std::mem::take(&mut this.render_again) {
                    this.render_page(cx);
                    return;
                }
                match result {
                    Ok(rendered) => {
                        if !this.compiling {
                            this.message = "".into();
                        }
                        let buffer = image::RgbaImage::from_raw(
                            rendered.width,
                            rendered.height,
                            rendered.bgra,
                        )
                        .expect("validated PDF raster dimensions");
                        let image =
                            Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
                                buffer
                            )]));
                        if let Some(old) = this.image.replace(image) {
                            cx.drop_image(old, None);
                        }
                        this.page = rendered.page;
                        this.page_count = rendered.count;
                    }
                    Err(error) => this.message = format!("{error:#}").into(),
                }
                cx.notify();
            })
            .ok();
        });
    }
}

impl Drop for LatexPreview {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Render for LatexPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("latex-preview")
            .track_focus(&self.focus)
            .size_full()
            .bg(cx.theme().colors().editor_background)
            .child(
                h_flex()
                    .p_2()
                    .gap_2()
                    .child(
                        Button::new("compile", "Rebuild")
                            .disabled(self.compiling)
                            .on_click(cx.listener(|this, _, _, cx| this.compile(cx))),
                    )
                    .child(
                        Button::new("previous", "Previous")
                            .disabled(self.page == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.page = this.page.saturating_sub(1);
                                this.render_page(cx);
                            })),
                    )
                    .child(Label::new(format!(
                        "{} / {}",
                        if self.page_count == 0 {
                            0
                        } else {
                            self.page + 1
                        },
                        self.page_count
                    )))
                    .child(
                        Button::new("next", "Next")
                            .disabled(self.page + 1 >= self.page_count)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.page = (this.page + 1).min(this.page_count.saturating_sub(1));
                                this.render_page(cx);
                            })),
                    )
                    .child(
                        Button::new("zoom-out", "−").on_click(cx.listener(|this, _, _, cx| {
                            this.zoom = (this.zoom / 1.25).max(0.25);
                            this.render_page(cx);
                        })),
                    )
                    .child(
                        Button::new("zoom-in", "+").on_click(cx.listener(|this, _, _, cx| {
                            this.zoom = (this.zoom * 1.25).min(3.0);
                            this.render_page(cx);
                        })),
                    ),
            )
            .child(
                div()
                    .id("latex-pages")
                    .flex_1()
                    .min_h_0()
                    .overflow_scroll()
                    .p_4()
                    .when(!self.message.is_empty(), |element| {
                        element.child(div().mb_2().child(self.message.clone()))
                    })
                    .when_some(self.image.clone(), |element, image| {
                        element.child(div().bg(gpui::white()).child(img(image)))
                    }),
            )
    }
}

impl Focusable for LatexPreview {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl EventEmitter<()> for LatexPreview {}
impl Item for LatexPreview {
    type Event = ();
    fn tab_content_text(&self, _: usize, _: &App) -> SharedString {
        format!(
            "{} — Preview",
            self.path.file_name().unwrap_or_default().to_string_lossy()
        )
        .into()
    }
}
