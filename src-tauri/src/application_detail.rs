use std::sync::Mutex;

use serde::Serialize;
use tauri::{Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use crate::error::{AppErrorPayload, CoreError};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Target {
    application_id: String,
    revision: u64,
}

#[derive(Default)]
pub(crate) struct DetailState(Mutex<Option<Target>>);

impl DetailState {
    fn set(&self, application_id: String) -> Result<Target, CoreError> {
        let mut current = self.0.lock().map_err(|_| CoreError::StateUnavailable)?;
        let revision = current
            .as_ref()
            .map_or(1, |value| value.revision.saturating_add(1));
        let target = Target {
            application_id,
            revision,
        };
        *current = Some(target.clone());
        Ok(target)
    }

    pub(crate) fn get(&self) -> Result<Option<Target>, CoreError> {
        self.0
            .lock()
            .map(|value| value.clone())
            .map_err(|_| CoreError::StateUnavailable)
    }
}

pub(crate) fn select<R: Runtime>(
    app: &tauri::AppHandle<R>,
    state: &DetailState,
    application_id: String,
    show: bool,
) -> Result<(), AppErrorPayload> {
    if application_id.trim().is_empty() {
        return Err(CoreError::Validation.into());
    }
    let target = state.set(application_id)?;
    if let Some(window) = app.get_webview_window("application-detail") {
        window
            .emit_to("application-detail", "application-detail-target", &target)
            .map_err(|_| CoreError::StateUnavailable)?;
        if show {
            window
                .unminimize()
                .map_err(|_| CoreError::StateUnavailable)?;
            window.show().map_err(|_| CoreError::StateUnavailable)?;
            window
                .set_focus()
                .map_err(|_| CoreError::StateUnavailable)?;
        }
    } else if show {
        let dev_url = app.config().build.dev_url.clone();
        WebviewWindowBuilder::new(
            app,
            "application-detail",
            WebviewUrl::App("index.html?window=application-detail".into()),
        )
        .title("OfferTrack 投递详情")
        .inner_size(560.0, 820.0)
        .min_inner_size(440.0, 600.0)
        .center()
        .menu(tauri::menu::Menu::new(app).map_err(|_| CoreError::StateUnavailable)?)
        .on_navigation(move |url| navigation_allowed(url, dev_url.as_ref(), cfg!(dev)))
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .build()
        .map_err(|_| CoreError::StateUnavailable)?;
    }
    Ok(())
}

fn navigation_allowed(url: &url::Url, dev: Option<&url::Url>, development: bool) -> bool {
    if url.path() != "/index.html"
        || url.query() != Some("window=application-detail")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    let bundled = url.port().is_none()
        && ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
            || (matches!(url.scheme(), "http" | "https")
                && url.host_str() == Some("tauri.localhost")));
    bundled || (development && dev.is_some_and(|base| url.origin() == base.origin()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_revision_advances_and_navigation_is_isolated() {
        let state = DetailState::default();
        let first = state.set("one".into()).unwrap();
        let second = state.set("two".into()).unwrap();
        assert!(second.revision > first.revision);
        assert_eq!(state.get().unwrap().unwrap().application_id, "two");

        let dev = url::Url::parse("http://127.0.0.1:1420").unwrap();
        assert!(navigation_allowed(
            &url::Url::parse("tauri://localhost/index.html?window=application-detail").unwrap(),
            None,
            false
        ));
        assert!(navigation_allowed(
            &url::Url::parse("http://127.0.0.1:1420/index.html?window=application-detail").unwrap(),
            Some(&dev),
            true
        ));
        assert!(!navigation_allowed(
            &url::Url::parse("tauri://localhost/index.html").unwrap(),
            None,
            false
        ));
    }
}
