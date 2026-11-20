// SPDX-License-Identifier: GPL-3.0-only
//! Single shared inference dispatch for the loaded model.
//!
//! Every transcription path — the one-shot `handle_transcribe`, the
//! push-to-talk recording flow, and the realtime streaming sessions — needs
//! the same primitive: run one inference pass against the currently-loaded
//! model, awaiting online (API) models directly and pushing local models onto
//! a blocking thread so their synchronous compute does not stall the async
//! runtime. That primitive lived inline (and drifted) in each path; it lives
//! here once.
//!
//! The caller owns the error/empty policy. [`dispatch_transcription`] returns
//! the raw outcome ([`DispatchError`] distinguishes "no model loaded", a
//! backend failure, and a panicked blocking task) so each path can keep its
//! own behavior — preview swallows failures to an empty string, the final
//! pass surfaces them, the one-shot reports an empty success.

use std::sync::Arc;
use super_voice_shared::models::contexts::ExtraContext;

use crate::daemon::types::SharedLoadedModel;

/// Why a single transcription dispatch did not produce text.
pub(crate) enum DispatchError {
    /// No model was loaded in the shared slot when the dispatch ran.
    NotLoaded,
    /// The backend's `transcribe_audio` call returned an error.
    Failed(anyhow::Error),
    /// The blocking inference task panicked or was cancelled.
    Join(tokio::task::JoinError),
}

/// Run one inference pass against the currently-loaded model.
///
/// `processed_audio` is the resampled/normalized audio ready for the backend;
/// `sample_rate` is its rate (callers pass `16000`). `language` is the resolved
/// BCP-47 tag or `None` to let the model use its primary language. `extra` is
/// the dictation's own context, added to the user's for this pass.
///
/// Online models are awaited on the async runtime; local models run on a
/// blocking thread. Returns the transcribed text, or a [`DispatchError`] the
/// caller maps to its own policy.
pub(crate) async fn dispatch_transcription(
    model: &SharedLoadedModel,
    processed_audio: Vec<f32>,
    sample_rate: u32,
    language: Option<String>,
    extra: Option<ExtraContext>,
) -> Result<String, DispatchError> {
    let is_online = {
        let guard = model.read().await;
        guard
            .as_ref()
            .is_some_and(|loaded| loaded.instance.is_online())
    };

    if is_online {
        // Online (API) models: await directly on the runtime.
        let mut guard = model.write().await;
        let Some(loaded) = guard.get_mut() else {
            return Err(DispatchError::NotLoaded);
        };
        loaded
            .instance
            .transcribe_audio(
                &processed_audio,
                sample_rate,
                language.as_deref(),
                extra.as_ref(),
            )
            .await
            .map_err(DispatchError::Failed)
    } else {
        // Local models: run the synchronous compute on a blocking thread so it
        // does not stall the async runtime.
        let model = Arc::clone(model);
        tokio::task::spawn_blocking(move || {
            let handle = tokio::runtime::Handle::current();
            let mut guard = model.blocking_write();
            let Some(loaded) = guard.get_mut() else {
                return Err(DispatchError::NotLoaded);
            };
            handle
                .block_on(loaded.instance.transcribe_audio(
                    &processed_audio,
                    sample_rate,
                    language.as_deref(),
                    extra.as_ref(),
                ))
                .map_err(DispatchError::Failed)
        })
        .await
        .map_err(DispatchError::Join)?
    }
}

/// Run one post-processing pass against the loaded post-processor.
///
/// The transcription twin of this function is [`dispatch_transcription`]; the
/// online/local split is the same and for the same reason — a cloud processor
/// is awaited on the runtime, a local one runs on a blocking thread so its
/// synchronous compute does not stall the async runtime.
///
/// Returns the rewritten text, or a [`DispatchError`] the caller maps to its
/// own policy. The daemon's policy is best-effort: see
/// [`SuperSTTDaemon::post_process_final`](crate::daemon::types::SuperSTTDaemon).
pub(crate) async fn dispatch_post_process(
    post_processor: &SharedLoadedModel,
    text: String,
    language: Option<String>,
    extra: Option<ExtraContext>,
) -> Result<String, DispatchError> {
    let is_online = {
        let guard = post_processor.read().await;
        guard
            .as_ref()
            .is_some_and(|loaded| loaded.instance.is_online())
    };

    if is_online {
        let mut guard = post_processor.write().await;
        let Some(loaded) = guard.get_mut() else {
            return Err(DispatchError::NotLoaded);
        };
        loaded
            .instance
            .process_text(&text, language.as_deref(), extra.as_ref())
            .await
            .map_err(DispatchError::Failed)
    } else {
        let post_processor = Arc::clone(post_processor);
        tokio::task::spawn_blocking(move || {
            let handle = tokio::runtime::Handle::current();
            let mut guard = post_processor.blocking_write();
            let Some(loaded) = guard.get_mut() else {
                return Err(DispatchError::NotLoaded);
            };
            handle
                .block_on(
                    loaded
                        .instance
                        .process_text(&text, language.as_deref(), extra.as_ref()),
                )
                .map_err(DispatchError::Failed)
        })
        .await
        .map_err(DispatchError::Join)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::types::LoadedModel;
    use crate::stt_models::ModelDefinition;
    use crate::stt_models::transcribe::{ModelInfo, ModelInfoData, ModelState, Transcribe};
    use std::sync::Mutex;
    use std::time::Duration;

    /// What the fake backend returns from `transcribe_audio`.
    #[derive(Clone)]
    enum Outcome {
        Ok(String),
        Err,
    }

    /// Configurable `Transcribe` fake. Records the args of the last call so
    /// tests can assert the dispatch forwards audio + language unchanged.
    #[allow(
        dead_code,
        reason = "fields are read by whichever dispatch a test drives"
    )]
    struct FakeModel {
        info: ModelInfoData,
        outcome: Outcome,
        // Outer `Option` records whether dispatch ran at all; inner is the
        // `language` argument it forwarded, which is itself optional.
        #[allow(clippy::option_option, reason = "outer = called, inner = the argument")]
        seen_language: Arc<Mutex<Option<Option<String>>>>,
        seen_audio_len: Arc<Mutex<Option<usize>>>,
    }

    impl ModelInfo for FakeModel {
        fn info(&self) -> &ModelInfoData {
            &self.info
        }
    }
    impl ModelState for FakeModel {
        fn device(&self) -> String {
            "cpu".to_string()
        }
    }
    #[async_trait::async_trait]
    impl Transcribe for FakeModel {
        async fn transcribe_audio(
            &mut self,
            audio: &[f32],
            _sample_rate: u32,
            language: Option<&str>,
            extra: Option<&ExtraContext>,
        ) -> anyhow::Result<String> {
            *self.seen_language.lock().unwrap() = Some(language.map(str::to_string));
            *self.seen_audio_len.lock().unwrap() = Some(audio.len());
            match &self.outcome {
                Outcome::Ok(text) => Ok(with_terms(text, extra)),
                Outcome::Err => anyhow::bail!("backend boom"),
            }
        }

        async fn process_text(
            &mut self,
            text: &str,
            language: Option<&str>,
            extra: Option<&ExtraContext>,
        ) -> anyhow::Result<String> {
            *self.seen_language.lock().unwrap() = Some(language.map(str::to_string));
            match &self.outcome {
                // Wrap the input so a test can tell a processed transcript from
                // a passed-through one.
                Outcome::Ok(marker) => Ok(with_terms(&format!("{marker}:{text}"), extra)),
                Outcome::Err => anyhow::bail!("processor boom"),
            }
        }
    }

    /// `text`, with the terms of the dictation's own context after it, so a
    /// test reads what reached the backend in what it answered.
    fn with_terms(text: &str, extra: Option<&ExtraContext>) -> String {
        match extra {
            Some(extra) => format!("{text} [{}]", extra.vocabulary.join(", ")),
            None => text.to_string(),
        }
    }

    type Probes = (
        SharedLoadedModel,
        Arc<Mutex<Option<Option<String>>>>,
        Arc<Mutex<Option<usize>>>,
    );

    async fn loaded(online: bool, outcome: Outcome) -> Probes {
        let seen_language = Arc::new(Mutex::new(None));
        let seen_audio_len = Arc::new(Mutex::new(None));
        let info = ModelInfoData::new(
            "fake",
            "github.com/x/y",
            true,
            online,
            Duration::from_secs(1),
        );
        let definition = ModelDefinition {
            name: "fake".to_string(),
            source: "github.com/x/y".to_string(),
            is_multilingual: true,
            primary_language: "en".to_string(),
            supported_languages: vec!["en".to_string()],
            estimated_vram_bytes: 0,
            processing_interval: Duration::from_secs(1),
            supported_devices: vec![super_voice_registry_types::manifest::Device::Cpu],
            realtime: false,
            product: super_voice_registry_types::manifest::VoiceModel {
                force_preview_support: true,
                ..super_voice_registry_types::manifest::VoiceModel::new(
                    super_voice_registry_types::manifest::ModelRole::Transcription,
                )
            },
            provider: None,
        };
        let instance = Box::new(FakeModel {
            info,
            outcome,
            seen_language: Arc::clone(&seen_language),
            seen_audio_len: Arc::clone(&seen_audio_len),
        });
        let model: SharedLoadedModel = Arc::default();
        crate::daemon::types::put_model(
            &model,
            LoadedModel {
                definition,
                instance,
            },
        )
        .await;
        (model, seen_language, seen_audio_len)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn online_model_returns_transcribed_text() {
        let (model, _, _) = loaded(true, Outcome::Ok("hello".to_string())).await;
        let out = dispatch_transcription(&model, vec![0.0; 16000], 16000, None, None).await;
        assert!(
            matches!(out, Ok(ref t) if t == "hello"),
            "online path should return the backend text"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_model_returns_transcribed_text() {
        // Exercises the spawn_blocking path used for non-online models.
        let (model, _, _) = loaded(false, Outcome::Ok("world".to_string())).await;
        let out = dispatch_transcription(&model, vec![0.0; 16000], 16000, None, None).await;
        assert!(
            matches!(out, Ok(ref t) if t == "world"),
            "local blocking path should return the backend text"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn online_backend_error_maps_to_failed() {
        let (model, _, _) = loaded(true, Outcome::Err).await;
        let out = dispatch_transcription(&model, vec![0.0; 16000], 16000, None, None).await;
        assert!(matches!(out, Err(DispatchError::Failed(_))));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_backend_error_maps_to_failed() {
        let (model, _, _) = loaded(false, Outcome::Err).await;
        let out = dispatch_transcription(&model, vec![0.0; 16000], 16000, None, None).await;
        assert!(matches!(out, Err(DispatchError::Failed(_))));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_model_loaded_maps_to_not_loaded() {
        let model: SharedLoadedModel = Arc::default();
        let out = dispatch_transcription(&model, vec![0.0; 16000], 16000, None, None).await;
        assert!(matches!(out, Err(DispatchError::NotLoaded)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn online_post_processor_returns_processed_text() {
        let (model, _, _) = loaded(true, Outcome::Ok("clean".to_string())).await;
        let out = dispatch_post_process(&model, "um hello".to_string(), None, None).await;
        assert!(
            matches!(out, Ok(ref t) if t == "clean:um hello"),
            "online path should return the processor's text"
        );
    }

    /// The blocking path exists so a local processor's synchronous compute does
    /// not stall the runtime; it must return the same text as the online one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_post_processor_returns_processed_text() {
        let (model, _, _) = loaded(false, Outcome::Ok("clean".to_string())).await;
        let out = dispatch_post_process(&model, "um hello".to_string(), None, None).await;
        assert!(matches!(out, Ok(ref t) if t == "clean:um hello"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn post_processor_error_maps_to_failed() {
        let (model, _, _) = loaded(true, Outcome::Err).await;
        let out = dispatch_post_process(&model, "hi".to_string(), None, None).await;
        assert!(matches!(out, Err(DispatchError::Failed(_))));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_post_processor_loaded_maps_to_not_loaded() {
        let model: SharedLoadedModel = Arc::default();
        let out = dispatch_post_process(&model, "hi".to_string(), None, None).await;
        assert!(matches!(out, Err(DispatchError::NotLoaded)));
    }

    /// The processor punctuates in the transcript's language, so the resolved
    /// tag has to reach it the same way it reaches the transcription backend.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn language_is_forwarded_to_the_post_processor() {
        let (model, seen_lang, _) = loaded(true, Outcome::Ok("clean".to_string())).await;
        let _ = dispatch_post_process(&model, "hola".to_string(), Some("es-MX".to_string()), None)
            .await;
        assert_eq!(
            seen_lang.lock().unwrap().clone(),
            Some(Some("es-MX".to_string()))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn language_and_audio_are_forwarded_to_backend() {
        let (model, seen_lang, seen_len) = loaded(true, Outcome::Ok("ok".to_string())).await;
        let _ = dispatch_transcription(
            &model,
            vec![0.0; 1234],
            16000,
            Some("es-MX".to_string()),
            None,
        )
        .await;
        assert_eq!(
            seen_lang.lock().unwrap().clone(),
            Some(Some("es-MX".to_string())),
            "the resolved language must reach the backend unchanged"
        );
        assert_eq!(*seen_len.lock().unwrap(), Some(1234));
    }

    /// A dictation's own context reaches both stages, as the language does.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dictations_context_reaches_both_stages() {
        let extra = ExtraContext {
            prompt: String::new(),
            vocabulary: vec!["agy".to_string()],
        };
        let (model, _, _) = loaded(true, Outcome::Ok("open it".to_string())).await;
        let heard =
            dispatch_transcription(&model, vec![0.0; 16], 16000, None, Some(extra.clone())).await;
        assert!(matches!(heard.as_deref(), Ok("open it [agy]")));
        let (processor, _, _) = loaded(false, Outcome::Ok("clean".to_string())).await;
        let processed =
            dispatch_post_process(&processor, "open it".to_string(), None, Some(extra)).await;
        assert!(matches!(processed.as_deref(), Ok("clean:open it [agy]")));
    }
}
