use crate::{settings_content::SettingsContent, settings_store::SettingsStore};
use collections::HashSet;
use fs::{Fs, PathEventKind};
use futures::{StreamExt, channel::mpsc};
use gpui::{App, BackgroundExecutor, ReadGlobal};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[cfg(test)]
mod tests {
    use super::*;
    use fs::FakeFs;

    use gpui::TestAppContext;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn inherited_config_preserves_overrides_and_keymap_order() {
        let merged = inherit_config(
            r#"{ // JSONC is supported
            "theme":"Official", "languages":{"Rust":{"tab_size":4}}, "buffer_font_size":15
        }"#,
            r#"{"languages":{"Rust":{"tab_size":2}}, "latex":{"preamble":"custom"}}"#,
            false,
        )
        .unwrap();
        let merged: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(merged["theme"], "Official");
        assert_eq!(merged["buffer_font_size"], 15);
        assert_eq!(merged["languages"]["Rust"]["tab_size"], 2);
        assert_eq!(merged["latex"]["preamble"], "custom");
        let keys = inherit_config(
            r#"[{"bindings":{"ctrl-a":"first"}}]"#,
            r#"[{"bindings":{"ctrl-a":"override"}}]"#,
            true,
        )
        .unwrap();
        let keys: serde_json::Value = serde_json::from_str(&keys).unwrap();
        assert_eq!(keys[1]["bindings"]["ctrl-a"], "override");
        assert!(inherit_config("{invalid", "{}", false).is_err());
        assert_eq!(inherit_config("", "{}", false).unwrap(), "{}");
    }

    #[gpui::test]
    async fn inherited_config_reloads_without_writing_official_files(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let fs = FakeFs::new(cx.background_executor.clone());
        let official = paths::official_config_dir().unwrap();
        let own = paths::config_dir();
        fs.insert_tree(
            &official,
            json!({"settings.json": r#"{"theme":"Official","buffer_font_size":15}"#}),
        )
        .await;
        fs.insert_tree(own, json!({"settings.json": r#"{"buffer_font_size":18}"#}))
            .await;
        let (mut rx, _watcher) = watch_config_file(
            &cx.background_executor,
            fs.clone(),
            own.join("settings.json"),
        );
        let initial: serde_json::Value = serde_json::from_str(&rx.next().await.unwrap()).unwrap();
        assert_eq!(initial["theme"], "Official");
        assert_eq!(initial["buffer_font_size"], 18);
        let changed = r#"{"theme":"Changed","buffer_font_size":16}"#;
        fs.insert_file(&official.join("settings.json"), changed.as_bytes().to_vec())
            .await;
        let updated: serde_json::Value = serde_json::from_str(&rx.next().await.unwrap()).unwrap();
        assert_eq!(updated["theme"], "Changed");
        assert_eq!(updated["buffer_font_size"], 18);
        fs.remove_file(&own.join("settings.json"), Default::default())
            .await
            .unwrap();
        let inherited: serde_json::Value = serde_json::from_str(&rx.next().await.unwrap()).unwrap();
        assert_eq!(inherited["buffer_font_size"], 16);
        assert_eq!(
            fs.load(&official.join("settings.json")).await.unwrap(),
            changed
        );
    }

    #[gpui::test]
    async fn test_watch_config_dir_reloads_tracked_file_on_rescan(cx: &mut TestAppContext) {
        cx.executor().allow_parking();

        let fs = FakeFs::new(cx.background_executor.clone());
        let config_dir = PathBuf::from("/root/config");
        let settings_path = PathBuf::from("/root/config/settings.json");

        fs.insert_tree(
            Path::new("/root"),
            json!({
                "config": {
                    "settings.json": "A"
                }
            }),
        )
        .await;

        let mut rx = watch_config_dir(
            &cx.background_executor,
            fs.clone(),
            config_dir.clone(),
            HashSet::from_iter([settings_path.clone()]),
        );

        assert_eq!(rx.next().await.as_deref(), Some("A"));
        cx.run_until_parked();

        fs.pause_events();
        fs.insert_file(&settings_path, b"B".to_vec()).await;
        fs.clear_buffered_events();

        fs.emit_fs_event(&settings_path, Some(PathEventKind::Rescan));
        fs.unpause_events_and_flush();
        assert_eq!(rx.next().await.as_deref(), Some("B"));

        fs.pause_events();
        fs.insert_file(&settings_path, b"A".to_vec()).await;
        fs.clear_buffered_events();

        fs.emit_fs_event(&config_dir, Some(PathEventKind::Rescan));
        fs.unpause_events_and_flush();
        assert_eq!(rx.next().await.as_deref(), Some("A"));
    }

    #[gpui::test]
    async fn test_watch_config_file_reloads_when_parent_dir_is_symlink(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let fs = FakeFs::new(cx.background_executor.clone());
        let config_settings_path = PathBuf::from("/root/.config/zed/settings.json");
        let target_settings_path = PathBuf::from("/root/dotfiles/zed/settings.json");

        fs.insert_tree(
            Path::new("/root"),
            json!({
                ".config": {},
                "dotfiles": {
                    "zed": {
                        "settings.json": "A"
                    }
                }
            }),
        )
        .await;

        fs.create_symlink(
            Path::new("/root/.config/zed"),
            PathBuf::from("/root/dotfiles/zed"),
        )
        .await
        .unwrap();

        let (mut rx, _task) =
            watch_config_file(&cx.background_executor, fs.clone(), config_settings_path);
        assert_eq!(rx.next().await.as_deref(), Some("A"));

        fs.insert_file(&target_settings_path, b"B".to_vec()).await;
        assert_eq!(rx.next().await.as_deref(), Some("B"));
    }
}

pub const EMPTY_THEME_NAME: &str = "empty-theme";

/// Settings for visual tests that use proper fonts instead of Courier.
/// Uses Helvetica Neue for UI (sans-serif) and Menlo for code (monospace),
/// which are available on all macOS systems.
#[cfg(any(test, feature = "test-support"))]
pub fn visual_test_settings() -> String {
    let mut value =
        crate::parse_json_with_comments::<serde_json::Value>(crate::default_settings().as_ref())
            .unwrap();
    util::merge_non_null_json_value_into(
        serde_json::json!({
            "ui_font_family": ".SystemUIFont",
            "ui_font_features": {},
            "ui_font_size": 14,
            "ui_font_fallback": [],
            "buffer_font_family": "Menlo",
            "buffer_font_features": {},
            "buffer_font_size": 14,
            "buffer_font_fallbacks": [],
            "theme": EMPTY_THEME_NAME,
        }),
        &mut value,
    );
    value.as_object_mut().unwrap().remove("languages");
    serde_json::to_string(&value).unwrap()
}

#[cfg(any(test, feature = "test-support"))]
pub fn test_settings() -> &'static str {
    static CACHED: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        let mut value = crate::parse_json_with_comments::<serde_json::Value>(
            crate::default_settings().as_ref(),
        )
        .unwrap();
        #[cfg(not(target_os = "windows"))]
        util::merge_non_null_json_value_into(
            serde_json::json!({
                "format_on_save": "on",
                "ui_font_family": "Courier",
                "ui_font_features": {},
                "ui_font_size": 14,
                "ui_font_fallback": [],
                "buffer_font_family": "Courier",
                "buffer_font_features": {},
                "buffer_font_size": 14,
                "buffer_font_fallbacks": [],
                "theme": EMPTY_THEME_NAME,
            }),
            &mut value,
        );
        #[cfg(target_os = "windows")]
        util::merge_non_null_json_value_into(
            serde_json::json!({
                "format_on_save": "on",
                "ui_font_family": "Courier New",
                "ui_font_features": {},
                "ui_font_size": 14,
                "ui_font_fallback": [],
                "buffer_font_family": "Courier New",
                "buffer_font_features": {},
                "buffer_font_size": 14,
                "buffer_font_fallbacks": [],
                "theme": EMPTY_THEME_NAME,
            }),
            &mut value,
        );
        value.as_object_mut().unwrap().remove("languages");
        serde_json::to_string(&value).unwrap()
    });
    &CACHED
}

pub fn watch_config_file(
    executor: &BackgroundExecutor,
    fs: Arc<dyn Fs>,
    path: PathBuf,
) -> (mpsc::UnboundedReceiver<String>, gpui::Task<()>) {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let inherited = (path.parent() == Some(paths::config_dir().as_path())
        && matches!(
            name,
            "settings.json" | "global_settings.json" | "keymap.json" | "tasks.json" | "debug.json"
        ))
    .then(paths::official_config_dir)
    .flatten()
    .map(|dir| dir.join(name));
    let Some(inherited) = inherited else {
        return watch_single_config_file(executor, fs, path);
    };
    let keymap = name == "keymap.json";
    let (mut base_rx, base_task) =
        watch_single_config_file(executor, fs.clone(), inherited.clone());
    let (mut own_rx, own_task) = watch_single_config_file(executor, fs, path);
    let (tx, rx) = mpsc::unbounded();
    let task = executor.spawn(async move {
        let _watchers = (base_task, own_task);
        let mut base = base_rx.next().await.unwrap_or_default();
        let mut own = own_rx.next().await.unwrap_or_default();
        let mut changes = futures::stream::select(
            base_rx.map(|content| (true, content)),
            own_rx.map(|content| (false, content)),
        );
        loop {
            match inherit_config(&base, &own, keymap) {
                Ok(content) => {
                    if tx.unbounded_send(content).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    log::error!(
                        "Could not inherit configuration from {}: {error}",
                        inherited.display()
                    );
                    // Let the existing parser report the error and retain its last good settings.
                    let invalid = if serde_json_lenient::from_str::<serde_json::Value>(&own)
                        .is_err()
                        && !own.trim().is_empty()
                    {
                        &own
                    } else {
                        &base
                    };
                    if tx.unbounded_send(invalid.clone()).is_err() {
                        break;
                    }
                }
            }
            let Some((is_base, content)) = changes.next().await else {
                break;
            };
            if is_base {
                base = content;
            } else {
                own = content;
            }
        }
    });
    (rx, task)
}

fn inherit_config(base: &str, own: &str, keymap: bool) -> anyhow::Result<String> {
    if base.trim().is_empty() {
        return Ok(own.to_owned());
    }
    if own.trim().is_empty() {
        return Ok(base.to_owned());
    }
    let mut base: serde_json::Value = serde_json_lenient::from_str(base)?;
    let own: serde_json::Value = serde_json_lenient::from_str(own)?;
    if keymap {
        let base = base
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("Official keymap must be an array"))?;
        base.extend(
            own.as_array()
                .ok_or_else(|| anyhow::anyhow!("Custom keymap must be an array"))?
                .iter()
                .cloned(),
        );
    } else {
        util::merge_json_value_into(own, &mut base);
    }
    Ok(serde_json::to_string(&base)?)
}

fn watch_single_config_file(
    executor: &BackgroundExecutor,
    fs: Arc<dyn Fs>,
    path: PathBuf,
) -> (mpsc::UnboundedReceiver<String>, gpui::Task<()>) {
    let (tx, rx) = mpsc::unbounded();
    let task = executor.spawn(async move {
        let path = fs.canonicalize(&path).await.unwrap_or_else(|_| path);
        let (events, _) = fs.watch(&path, Duration::from_millis(100)).await;
        futures::pin_mut!(events);

        let contents = fs.load(&path).await.unwrap_or_default();
        if tx.unbounded_send(contents).is_err() {
            return;
        }

        loop {
            if events.next().await.is_none() {
                break;
            }

            let contents = match fs.load(&path).await {
                Ok(contents) => contents,
                Err(_)
                    if fs
                        .metadata(&path)
                        .await
                        .is_ok_and(|metadata| metadata.is_none()) =>
                {
                    String::new()
                }
                Err(_) => continue,
            };
            if tx.unbounded_send(contents).is_err() {
                break;
            }
        }
    });
    (rx, task)
}

pub fn watch_config_dir(
    executor: &BackgroundExecutor,
    fs: Arc<dyn Fs>,
    dir_path: PathBuf,
    config_paths: HashSet<PathBuf>,
) -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded();
    executor
        .spawn(async move {
            for file_path in &config_paths {
                if fs.metadata(file_path).await.is_ok_and(|v| v.is_some())
                    && let Ok(contents) = fs.load(file_path).await
                    && tx.unbounded_send(contents).is_err()
                {
                    return;
                }
            }

            let (events, _) = fs.watch(&dir_path, Duration::from_millis(100)).await;
            futures::pin_mut!(events);

            while let Some(event_batch) = events.next().await {
                for event in event_batch {
                    if config_paths.contains(&event.path) {
                        match event.kind {
                            Some(PathEventKind::Removed) => {
                                if tx.unbounded_send(String::new()).is_err() {
                                    return;
                                }
                            }
                            Some(PathEventKind::Created) | Some(PathEventKind::Changed) => {
                                if let Ok(contents) = fs.load(&event.path).await
                                    && tx.unbounded_send(contents).is_err()
                                {
                                    return;
                                }
                            }
                            Some(PathEventKind::Rescan) => {
                                for file_path in &config_paths {
                                    if let Ok(contents) = fs.load(file_path).await
                                        && tx.unbounded_send(contents).is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                            _ => {}
                        }
                    } else if matches!(event.kind, Some(PathEventKind::Rescan))
                        && event.path == dir_path
                    {
                        for file_path in &config_paths {
                            if let Ok(contents) = fs.load(file_path).await
                                && tx.unbounded_send(contents).is_err()
                            {
                                return;
                            }
                        }
                    }
                }
            }
        })
        .detach();

    rx
}

pub fn update_settings_file(
    fs: Arc<dyn Fs>,
    cx: &App,
    update: impl 'static + Send + FnOnce(&mut SettingsContent, &App),
) {
    SettingsStore::global(cx).update_settings_file(fs, update)
}

pub fn update_settings_file_with_completion(
    fs: Arc<dyn Fs>,
    cx: &App,
    update: impl 'static + Send + FnOnce(&mut SettingsContent, &App),
) -> futures::channel::oneshot::Receiver<anyhow::Result<()>> {
    SettingsStore::global(cx).update_settings_file_with_completion(fs, update)
}
