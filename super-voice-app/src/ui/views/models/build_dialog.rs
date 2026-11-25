// SPDX-License-Identifier: GPL-3.0-only

//! The question asked before downloading another build of a backend, so a
//! model can run on a framework the installed build does not.

use cosmic::Element;
use cosmic::widget::{self, button};

use super_voice_app_core::pages::models::{Input, PendingBuild};

use super::active::models;
use crate::core::app::AppModel;
use crate::ui::messages::Message;

/// The dialog for the framework picked whose build is not installed, while
/// the user has not answered it.
pub fn build_dialog(app: &AppModel) -> Option<Element<'_, Message>> {
    let pending = app.models.build_question()?;
    let (title, body) = build_question(app, pending);
    Some(
        widget::dialog()
            .title(title)
            .body(body)
            .primary_action(
                button::suggested(crate::fl!("download")).on_press(models(Input::ConfirmBuild)),
            )
            .secondary_action(
                button::standard(crate::fl!("cancel")).on_press(models(Input::CancelBuild)),
            )
            .into(),
    )
}

/// The dialog's title and body for `pending`.
fn build_question(app: &AppModel, pending: &PendingBuild) -> (String, String) {
    let backend = app
        .models
        .catalog
        .backend(pending.stage, &pending.source)
        .map_or(pending.source.as_str(), |b| b.name.as_str());
    // The size of that build for this machine, when the registry lists it.
    let size = app
        .registry
        .build(&pending.source, &pending.framework)
        .map(|build| build.size)
        .filter(|size| *size > 0);
    let running = loaded_here(app, pending);
    question(
        backend,
        &super::fmt::accel_name(&pending.framework),
        size,
        running,
    )
}

/// Whether the stage's model is running, so it reloads once the build is in.
fn loaded_here(app: &AppModel, pending: &PendingBuild) -> bool {
    let view = app.models.view(pending.stage);
    view.loaded && view.source.as_deref() == Some(pending.source.as_str())
}

/// The words, apart from the state they are read from.
fn question(backend: &str, framework: &str, size: Option<u64>, running: bool) -> (String, String) {
    let title = crate::fl!("download-the-build-question", framework = framework);
    let mut sentences = vec![crate::fl!(
        "backend-needs-its-build",
        backend = backend,
        framework = framework
    )];
    if let Some(size) = size {
        sentences.push(crate::fl!(
            "the-download-is-size",
            size = super::fmt::fmt_size(size)
        ));
    }
    sentences.push(crate::fl!("your-downloaded-models-are-kept"));
    if running {
        sentences.push(crate::fl!(
            "the-model-restarts-on-framework",
            framework = framework
        ));
    }
    (title, sentences.join(" "))
}

#[cfg(test)]
mod tests {
    use crate::i18n::without_isolation;

    fn question(
        backend: &str,
        framework: &str,
        size: Option<u64>,
        running: bool,
    ) -> (String, String) {
        let (title, body) = super::question(backend, framework, size, running);
        (without_isolation(&title), without_isolation(&body))
    }

    #[test]
    fn the_question_names_the_build_and_its_size() {
        let (title, body) = question("Qwen3 TTS", "CUDA", Some(1536 * 1024 * 1024), true);
        assert_eq!(title, "Download the CUDA build?");
        assert_eq!(
            body,
            "Qwen3 TTS needs its CUDA build to run on CUDA. The download is 1.5 GB. \
             Your downloaded models are kept. The model restarts on CUDA when it is done."
        );
    }

    #[test]
    fn an_unknown_size_and_a_model_not_running_are_left_out() {
        let (_, body) = question("Whisper", "Vulkan", None, false);
        assert_eq!(
            body,
            "Whisper needs its Vulkan build to run on Vulkan. Your downloaded models are kept."
        );
    }
}
