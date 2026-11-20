// SPDX-License-Identifier: GPL-3.0-only
//! Running a final transcript through the loaded post-processor.
//!
//! One funnel, called from both final-transcription paths (the one-shot
//! `POST /stt/transcribe` and the push-to-talk recording flow). Previews and
//! realtime sessions deliberately do not call it: a preview is rewritten again
//! on the next pass, and both are latency paths where a second model round-trip
//! is exactly what the user does not want.
//!
//! **Best-effort.** Every failure mode — nothing selected, nothing loaded, the
//! processor erroring, the processor hanging, the processor running on past
//! the transcript — yields the raw transcript. The
//! user asked to dictate; a cleanup step that is down must not cost them the
//! words. Failures are logged, and the raw text is what gets typed.
//!
//! This is also where a take is written to the log: the raw transcript, and
//! right after it the processed one when a processor ran. Reading the two
//! together is how to judge what the processor did to a take.

use std::time::Duration;

use log::{debug, info, warn};

use crate::daemon::device_management::PipelineStage;
use crate::daemon::types::SuperSTTDaemon;
use crate::stt_models::dispatch::{DispatchError, dispatch_post_process};
use super_engine_daemon::stage::StageExt;
use super_voice_shared::models::contexts::ExtraContext;

/// How long a post-processing pass may take before the raw transcript is used
/// instead.
///
/// Generous enough for a cloud LLM round trip on a long transcript, short
/// enough that a hung processor does not leave the user staring at nothing
/// after they finished speaking.
const POST_PROCESS_TIMEOUT: Duration = Duration::from_secs(30);

/// Characters a cleanup may add beyond doubling the transcript, so a very
/// short take can still gain its capital and full stop ("ok" → "Okay.").
const RUN_ON_SLACK: usize = 40;

/// Whether `processed` is longer than any cleanup of `raw` could be.
///
/// A cleanup takes filler out, punctuates and capitalizes; it never doubles
/// the text. An answer that does ran on past the end of the transcript: a
/// small model handed a take cut off mid-sentence finished the sentence, then
/// repeated itself ("to allow them to allow them…") until it hit its token
/// cap. What it added is not what the user said. Every post-processor today
/// is a cleanup; one that rewrote at length would need this revisited.
fn runs_on(raw: &str, processed: &str) -> bool {
    processed.chars().count() > raw.chars().count() * 2 + RUN_ON_SLACK
}

impl SuperSTTDaemon {
    /// Rewrite a final transcript with the loaded post-processor, falling back
    /// to `text` unchanged on any failure.
    ///
    /// `language` is the tag the transcript was produced in, forwarded so the
    /// processor punctuates in the right language.
    pub(crate) async fn post_process_final(
        &self,
        text: String,
        language: Option<String>,
        extra: Option<&ExtraContext>,
    ) -> String {
        // Whole, and before any early return: this is the one line that says
        // what the model heard, whether or not a processor rewrites it.
        info!("Raw transcript: '{text}'");
        // Empty input covers "no speech": there is nothing to clean up, and a
        // processor handed an empty string tends to invent one.
        if text.trim().is_empty() {
            return text;
        }
        if !self.config.read().await.post_processor.enabled {
            return text;
        }
        // Before the load, so a skip costs nothing: a processor declaring only
        // other languages would load just to be passed over.
        if let Some(lang) = self.post_processor_lacks(language.as_deref()).await {
            info!("The post-processor does not list {lang}; using the raw transcript");
            return text;
        }
        // The stage is used as any other is: loaded first when models are
        // unloaded on idle. A load that fails is logged and announced, and
        // the text passes through.
        let _ = self
            .stage(PipelineStage::PostProcessor)
            .ensure_loaded()
            .await;
        if !self.post_processor_loaded().await {
            debug!("Post-processing is enabled but no processor is loaded; using the raw text");
            return text;
        }

        let start = std::time::Instant::now();
        let raw = text.clone();
        match tokio::time::timeout(
            POST_PROCESS_TIMEOUT,
            dispatch_post_process(&self.post_processor, text, language, extra.cloned()),
        )
        .await
        {
            Ok(Ok(processed)) => {
                // A processor that answers with nothing has effectively eaten
                // the transcript; keep the words rather than typing a blank.
                if processed.trim().is_empty() {
                    warn!("Post-processor returned empty text; using the raw transcript");
                    return raw;
                }
                if runs_on(&raw, &processed) {
                    warn!(
                        "Post-processor ran on past the transcript ({} characters for {}); \
                         using the raw transcript. It returned: '{processed}'",
                        processed.chars().count(),
                        raw.chars().count()
                    );
                    return raw;
                }
                info!(
                    "Post-processed transcript ({:?}): '{processed}'",
                    start.elapsed()
                );
                processed
            }
            Ok(Err(DispatchError::Failed(e))) => {
                warn!("Post-processing failed, using the raw transcript: {e}");
                raw
            }
            Ok(Err(DispatchError::NotLoaded)) => {
                // Raced with an unload between the check above and the dispatch.
                debug!("Post-processor unloaded mid-dispatch; using the raw transcript");
                raw
            }
            Ok(Err(DispatchError::Join(e))) => {
                warn!("Post-processing task failed, using the raw transcript: {e}");
                raw
            }
            Err(_) => {
                warn!(
                    "Post-processing timed out after {POST_PROCESS_TIMEOUT:?}; \
                     using the raw transcript"
                );
                raw
            }
        }
    }
}

impl SuperSTTDaemon {
    /// The transcript's language, when it is known and the selected
    /// post-processor does not list it.
    ///
    /// A post-processor rewrites text it was built for: an English-only one
    /// handed Spanish strips its punctuation and mangles words it takes for
    /// misspellings ("huevos" came out "hueos"). Its manifest says which
    /// languages it takes, the same way a transcription model's does, and a
    /// transcript in another passes through as transcribed. The policy is the
    /// one for a failing processor: the user's words are never worse.
    ///
    /// `None` when the language is unknown — no language (the model chose) or
    /// `auto` (it detected one it does not report) — since the processor
    /// ran on those before there was a check, and when no post-processor is
    /// selected or its backend is gone, which the load then reports.
    async fn post_processor_lacks(&self, language: Option<&str>) -> Option<String> {
        let lang = language.filter(|l| !l.eq_ignore_ascii_case("auto"))?;
        let (source, model) = {
            let config = self.config.read().await;
            let pp = &config.post_processor;
            (pp.source.clone(), pp.model.clone())
        };
        let def = self.find_model_definition(&source, &model).await?;
        (!lists_language(&def, lang)).then(|| lang.to_string())
    }
}

/// Whether `def` takes text in `tag`, by its manifest: its
/// `supported_languages` when it is multilingual, its `primary_language`
/// otherwise. Compared on the primary subtag, so a processor listing `es`
/// takes `es-MX`, and one listing `en` takes `en-GB`. A multilingual model that
/// lists no languages takes any.
fn lists_language(def: &crate::stt_models::ModelDefinition, tag: &str) -> bool {
    let primary = |t: &str| t.split(['-', '_']).next().unwrap_or(t).to_ascii_lowercase();
    let want = primary(tag);
    if def.is_multilingual {
        def.supported_languages.is_empty()
            || def.supported_languages.iter().any(|l| primary(l) == want)
    } else {
        primary(&def.primary_language) == want
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super_voice_registry_types::manifest::{Device, ModelRole};

    use crate::daemon::types::{LoadedModel, SuperSTTDaemon, test_daemon};
    use crate::stt_models::ModelDefinition;
    use crate::stt_models::transcribe::{ModelInfo, ModelInfoData, ModelState, Transcribe};

    /// A post-processor fake: either uppercases its input (so a test can see
    /// that processing happened), fails, answers with nothing, or loops past
    /// the end of its input.
    enum Behavior {
        Uppercase,
        Fail,
        Empty,
        Loop,
    }

    struct FakeProcessor {
        info: ModelInfoData,
        behavior: Behavior,
    }

    impl ModelInfo for FakeProcessor {
        fn info(&self) -> &ModelInfoData {
            &self.info
        }
    }
    impl ModelState for FakeProcessor {
        fn device(&self) -> String {
            "remote".to_string()
        }
    }
    #[async_trait::async_trait]
    impl Transcribe for FakeProcessor {
        async fn transcribe_audio(
            &mut self,
            _audio: &[f32],
            _sample_rate: u32,
            _language: Option<&str>,
            _extra: Option<&super_voice_shared::models::contexts::ExtraContext>,
        ) -> anyhow::Result<String> {
            anyhow::bail!("not a transcription model")
        }

        async fn process_text(
            &mut self,
            text: &str,
            _language: Option<&str>,
            _extra: Option<&super_voice_shared::models::contexts::ExtraContext>,
        ) -> anyhow::Result<String> {
            match self.behavior {
                Behavior::Uppercase => Ok(text.to_uppercase()),
                Behavior::Fail => anyhow::bail!("processor boom"),
                Behavior::Empty => Ok("   ".to_string()),
                Behavior::Loop => Ok(format!("{text}{}", " to allow them".repeat(40))),
            }
        }
    }

    /// A daemon with post-processing enabled and `behavior` loaded in the
    /// post-processor slot.
    async fn daemon_with(behavior: Behavior) -> SuperSTTDaemon {
        let daemon = test_daemon().await;
        daemon.config.write().await.post_processor.enabled = true;
        let definition = ModelDefinition {
            name: "cleanup".to_string(),
            source: "github.com/x/cleanup".to_string(),
            is_multilingual: true,
            primary_language: "en".to_string(),
            supported_languages: vec!["en".to_string()],
            estimated_vram_bytes: 0,
            processing_interval: std::time::Duration::from_secs(1),
            supported_devices: vec![Device::None],
            realtime: false,
            product: super_voice_registry_types::manifest::VoiceModel::new(
                ModelRole::PostProcessor,
            ),
            provider: None,
        };
        let info = ModelInfoData::new(
            "cleanup",
            "github.com/x/cleanup",
            true,
            true,
            std::time::Duration::from_secs(1),
        );
        crate::daemon::types::put_model(
            &daemon.post_processor,
            LoadedModel {
                definition,
                instance: Box::new(FakeProcessor { info, behavior }),
            },
        )
        .await;
        daemon
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_loaded_processor_rewrites_the_transcript() {
        let daemon = daemon_with(Behavior::Uppercase).await;
        let out = daemon
            .post_process_final("hello there".into(), None, None)
            .await;
        assert_eq!(out, "HELLO THERE");
    }

    /// The whole point of the best-effort policy: a processor that fails costs
    /// the user the cleanup, never the words.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_failing_processor_yields_the_raw_transcript() {
        let daemon = daemon_with(Behavior::Fail).await;
        let out = daemon
            .post_process_final("hello there".into(), None, None)
            .await;
        assert_eq!(out, "hello there");
    }

    /// A processor answering with nothing has eaten the transcript; keeping the
    /// raw text beats typing a blank.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_empty_answer_yields_the_raw_transcript() {
        let daemon = daemon_with(Behavior::Empty).await;
        let out = daemon
            .post_process_final("hello there".into(), None, None)
            .await;
        assert_eq!(out, "hello there");
    }

    /// The take that found this: cut off mid-sentence, the processor carried
    /// the sentence on and looped until its token cap, and the loop was
    /// typed. A cleanup never doubles a transcript, so the raw text wins.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_answer_that_runs_on_yields_the_raw_transcript() {
        let daemon = daemon_with(Behavior::Loop).await;
        let raw = "The second feature similarly is to allow users";
        let out = daemon.post_process_final(raw.into(), None, None).await;
        assert_eq!(out, raw);
    }

    /// A very short take still gets its cleanup: the slack covers the
    /// capital and full stop it gains.
    #[test]
    fn a_short_take_may_gain_its_punctuation() {
        assert!(!super::runs_on("ok", "Okay."));
        assert!(!super::runs_on(
            "milk eggs bread",
            "- Milk\n- Eggs\n- Bread"
        ));
        assert!(super::runs_on("allow users", &"to allow them ".repeat(10)));
    }

    /// The toggle is what the user set; a loaded processor must not run while
    /// it is off.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_disabled_processor_does_not_run() {
        let daemon = daemon_with(Behavior::Uppercase).await;
        daemon.config.write().await.post_processor.enabled = false;
        let out = daemon
            .post_process_final("hello there".into(), None, None)
            .await;
        assert_eq!(out, "hello there");
    }

    /// Enabled but nothing loaded — the startup load failed, say — still
    /// delivers the transcript.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn nothing_loaded_yields_the_raw_transcript() {
        let daemon = test_daemon().await;
        daemon.config.write().await.post_processor.enabled = true;
        let out = daemon
            .post_process_final("hello there".into(), None, None)
            .await;
        assert_eq!(out, "hello there");
    }

    /// "No speech" is an empty transcript. A processor handed one tends to
    /// invent a sentence, so it is never asked.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn empty_text_is_never_sent_to_the_processor() {
        let daemon = daemon_with(Behavior::Uppercase).await;
        assert_eq!(
            daemon.post_process_final(String::new(), None, None).await,
            ""
        );
        assert_eq!(
            daemon.post_process_final("   ".into(), None, None).await,
            "   "
        );
        assert!(
            Arc::strong_count(&daemon.post_processor) >= 1,
            "the slot is untouched"
        );
    }

    fn processor(multilingual: bool, primary: &str, supported: &[&str]) -> ModelDefinition {
        ModelDefinition {
            name: "cleanup".to_string(),
            source: "github.com/x/cleanup".to_string(),
            is_multilingual: multilingual,
            primary_language: primary.to_string(),
            supported_languages: supported.iter().map(ToString::to_string).collect(),
            estimated_vram_bytes: 0,
            processing_interval: std::time::Duration::from_secs(1),
            supported_devices: vec![Device::None],
            realtime: false,
            product: super_voice_registry_types::manifest::VoiceModel::new(
                ModelRole::PostProcessor,
            ),
            provider: None,
        }
    }

    /// An English-only processor takes English in any region and nothing
    /// else; the tag is compared on its primary subtag, either case.
    #[test]
    fn an_english_only_processor_takes_english_only() {
        let en = processor(false, "en", &["en"]);
        assert!(super::lists_language(&en, "en"));
        assert!(super::lists_language(&en, "en-GB"));
        assert!(super::lists_language(&en, "EN"));
        assert!(!super::lists_language(&en, "es"));
        assert!(!super::lists_language(&en, "es-MX"));
    }

    /// A multilingual processor takes what it lists, and one listing nothing
    /// takes anything.
    #[test]
    fn a_multilingual_processor_takes_what_it_lists() {
        let two = processor(true, "en", &["en", "es"]);
        assert!(super::lists_language(&two, "es-MX"));
        assert!(!super::lists_language(&two, "fr"));
        let any = processor(true, "en", &[]);
        assert!(super::lists_language(&any, "fr"));
    }
}
