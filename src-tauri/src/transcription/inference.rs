use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use whisper_rs::{FullParams, SamplingStrategy};

use crate::db::{Db, Language, NewSegment, Segment};
use crate::error::{AppError, DB_ERROR, WHISPER_ERROR};
use crate::settings::{GpuMode, SettingsStore};
use crate::transcription::context::{model_path, WhisperContextManager};
use crate::transcription::jobs::{JobKind, TranscribeJob};
use crate::transcription::worker::{JobProcessResult, JobProcessor};

const INITIAL_PROMPT_MAX_CHARS: usize = 200;
const MAX_TRANSCRIPTION_THREADS: usize = 8;
const NOISE_TEXT_PATTERNS: &[&str] = &[
    "(音楽)",
    "（音楽）",
    "[音楽]",
    "【音楽】",
    "(拍手)",
    "（拍手）",
    "[拍手]",
    "【拍手】",
    "[BLANK_AUDIO]",
    "(BLANK_AUDIO)",
    "ご視聴ありがとうございました",
    "ご視聴ありがとうございました。",
];

pub struct WhisperJobProcessor {
    db: Arc<Db>,
    context_manager: Arc<WhisperContextManager>,
    models_dir: PathBuf,
    settings_store: SettingsStore,
    recording_active: Arc<AtomicBool>,
}

impl WhisperJobProcessor {
    pub fn new(
        db: Arc<Db>,
        context_manager: Arc<WhisperContextManager>,
        models_dir: PathBuf,
        settings_store: SettingsStore,
        recording_active: Arc<AtomicBool>,
    ) -> Self {
        Self {
            db,
            context_manager,
            models_dir,
            settings_store,
            recording_active,
        }
    }

    fn current_gpu_mode(&self) -> Result<GpuMode, AppError> {
        Ok(self.settings_store.read()?.gpu_mode)
    }
}

impl JobProcessor for WhisperJobProcessor {
    fn process(
        &self,
        job: &TranscribeJob,
        should_abort: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<JobProcessResult, AppError> {
        if job.kind == JobKind::Batch && should_abort() {
            return Ok(JobProcessResult::Aborted);
        }

        let previous_segments = segments_for_session(&self.db, &job.session_id)?;
        let prompt = initial_prompt_from_segments(&previous_segments);
        self.context_manager.ensure_loaded(
            &job.model,
            &model_path(&self.models_dir, &job.model),
            self.current_gpu_mode()?,
        )?;

        let result = self
            .context_manager
            .with_context(&job.model, |context| {
                let mut state = context
                    .create_state()
                    .map_err(|error| whisper_error(format!("failed to create state: {error}")))?;
                let params = build_full_params(
                    job,
                    prompt.as_deref(),
                    Some(Arc::clone(&self.recording_active)),
                );
                if let Err(error) = state.full(params, &job.audio) {
                    if job.kind == JobKind::Batch && should_abort() {
                        return Ok(JobProcessResult::Aborted);
                    }
                    return Err(whisper_error(format!(
                        "failed to transcribe audio: {error}"
                    )));
                }
                if job.kind == JobKind::Batch && should_abort() {
                    return Ok(JobProcessResult::Aborted);
                }

                Ok(JobProcessResult::Completed {
                    segments: prepare_segments_for_insert(
                        &previous_segments,
                        job,
                        segments_from_state(job, &state)?,
                    ),
                })
            })
            .ok_or_else(|| whisper_error("whisper context was not loaded"))??;

        Ok(result)
    }
}

pub fn build_full_params(
    job: &TranscribeJob,
    initial_prompt: Option<&str>,
    recording_active: Option<Arc<AtomicBool>>,
) -> FullParams<'static, 'static> {
    let mut params: FullParams<'static, 'static> =
        FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(language_code(job.language));
    params.set_translate(false);
    params.set_no_context(true);
    params.set_suppress_blank(true);
    params.set_token_timestamps(false);
    params.set_n_threads(transcription_thread_count(
        std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1),
    ));
    if let Some(prompt) = initial_prompt.filter(|prompt| !prompt.is_empty()) {
        params.set_initial_prompt(prompt);
    }
    if job.kind == JobKind::Batch {
        if let Some(recording_active) = recording_active {
            let abort_callback: Box<dyn FnMut() -> bool> =
                Box::new(move || recording_active.load(Ordering::SeqCst));
            params.set_abort_callback_safe::<Option<Box<dyn FnMut() -> bool>>, Box<dyn FnMut() -> bool>>(
                Some(abort_callback),
            );
        }
    }
    params
}

pub fn initial_prompt_from_segments(segments: &[Segment]) -> Option<String> {
    let text = segments
        .iter()
        .map(|segment| segment.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        None
    } else {
        Some(tail_chars(&text, INITIAL_PROMPT_MAX_CHARS))
    }
}

pub fn filter_hallucinated_segments(segments: Vec<NewSegment>) -> Vec<NewSegment> {
    filter_hallucinated_segments_with_history(&[], segments)
}

pub fn filter_hallucinated_segments_with_history(
    previous_segments: &[Segment],
    segments: Vec<NewSegment>,
) -> Vec<NewSegment> {
    let mut filtered = Vec::with_capacity(segments.len());
    let (mut previous_text, mut repeat_count) = consecutive_tail_repeat(previous_segments);

    for segment in segments {
        let normalized = normalize_for_filter(&segment.text);
        if normalized.is_empty() || is_noise_text(&normalized) {
            continue;
        }

        if normalized == previous_text {
            repeat_count += 1;
        } else {
            previous_text = normalized;
            repeat_count = 1;
        }

        if repeat_count >= 3 {
            continue;
        }

        filtered.push(segment);
    }

    filtered
}

pub fn prepare_segments_for_insert(
    previous_segments: &[Segment],
    job: &TranscribeJob,
    segments: Vec<NewSegment>,
) -> Vec<NewSegment> {
    suppress_similar_overlaps(
        previous_segments,
        filter_hallucinated_segments_with_history(
            previous_segments,
            clip_segments_to_valid_range(job, segments),
        ),
    )
}

pub fn clip_segments_to_valid_range(
    job: &TranscribeJob,
    segments: Vec<NewSegment>,
) -> Vec<NewSegment> {
    let valid_start_ms = job.valid_start_ms as i64;
    let valid_end_ms = job.valid_end_ms as i64;
    segments
        .into_iter()
        .filter_map(|mut segment| {
            let center_ms = segment_center_ms(&segment);
            if center_ms < valid_start_ms || center_ms >= valid_end_ms {
                return None;
            }

            segment.start_ms = segment.start_ms.max(valid_start_ms);
            segment.end_ms = segment.end_ms.min(valid_end_ms);
            (segment.start_ms < segment.end_ms).then_some(segment)
        })
        .collect()
}

pub fn suppress_similar_overlaps(
    previous_segments: &[Segment],
    segments: Vec<NewSegment>,
) -> Vec<NewSegment> {
    let mut accepted = Vec::with_capacity(segments.len());
    let mut last_segment = previous_segments.last().cloned();

    for segment in segments {
        if last_segment.as_ref().is_some_and(|last| {
            overlaps_previous(last, &segment) && has_similar_text(last, &segment)
        }) {
            continue;
        }

        last_segment = Some(Segment {
            id: 0,
            session_id: segment.session_id.clone(),
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text: segment.text.clone(),
            lang: segment.lang.clone(),
        });
        accepted.push(segment);
    }

    accepted
}

fn segments_for_session(db: &Db, session_id: &str) -> Result<Vec<Segment>, AppError> {
    db.list_segments(session_id)
        .map_err(|error| AppError::new(DB_ERROR, format!("database error: {error}")))
}

fn segments_from_state(
    job: &TranscribeJob,
    state: &whisper_rs::WhisperState,
) -> Result<Vec<NewSegment>, AppError> {
    let mut segments = Vec::new();
    for segment in state.as_iter() {
        let text = segment
            .to_str_lossy()
            .map_err(|error| whisper_error(format!("failed to read segment text: {error}")))?
            .trim()
            .to_string();
        segments.push(NewSegment {
            session_id: job.session_id.clone(),
            start_ms: centiseconds_to_session_ms(job.chunk_start_ms, segment.start_timestamp()),
            end_ms: centiseconds_to_session_ms(job.chunk_start_ms, segment.end_timestamp()),
            text,
            lang: language_tag(job.language).map(str::to_string),
        });
    }
    Ok(segments)
}

fn centiseconds_to_session_ms(chunk_start_ms: u64, timestamp_cs: i64) -> i64 {
    chunk_start_ms as i64 + timestamp_cs.saturating_mul(10)
}

fn language_code(language: Language) -> Option<&'static str> {
    match language {
        Language::Ja => Some("ja"),
        Language::En => Some("en"),
        Language::Auto => None,
    }
}

fn language_tag(language: Language) -> Option<&'static str> {
    match language {
        Language::Ja => Some("ja"),
        Language::En => Some("en"),
        Language::Auto => None,
    }
}

fn transcription_thread_count(available_threads: usize) -> i32 {
    available_threads.clamp(1, MAX_TRANSCRIPTION_THREADS) as i32
}

fn tail_chars(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    text.chars()
        .skip(char_count.saturating_sub(max_chars))
        .collect()
}

fn normalize_for_filter(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

fn is_noise_text(normalized_text: &str) -> bool {
    NOISE_TEXT_PATTERNS
        .iter()
        .map(|pattern| normalize_for_filter(pattern))
        .any(|pattern| pattern == normalized_text)
}

fn consecutive_tail_repeat(segments: &[Segment]) -> (String, usize) {
    let mut iter = segments
        .iter()
        .rev()
        .map(|segment| normalize_for_filter(&segment.text))
        .filter(|text| !text.is_empty() && !is_noise_text(text));

    let Some(previous_text) = iter.next() else {
        return (String::new(), 0);
    };
    let repeat_count = 1 + iter.take_while(|text| text == &previous_text).count();
    (previous_text, repeat_count)
}

fn segment_center_ms(segment: &NewSegment) -> i64 {
    segment.start_ms + (segment.end_ms - segment.start_ms) / 2
}

fn overlaps_previous(previous: &Segment, segment: &NewSegment) -> bool {
    segment.start_ms < previous.end_ms
}

fn has_similar_text(previous: &Segment, segment: &NewSegment) -> bool {
    let previous_text = normalize_for_filter(&previous.text);
    let segment_text = normalize_for_filter(&segment.text);
    !previous_text.is_empty()
        && !segment_text.is_empty()
        && (previous_text == segment_text
            || previous_text.contains(&segment_text)
            || segment_text.contains(&previous_text))
}

fn whisper_error(message: impl Into<String>) -> AppError {
    AppError::new(WHISPER_ERROR, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewSegment;
    use crate::settings::{SettingsPatch, SettingsStore};
    use std::sync::atomic::AtomicBool;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn prompt_uses_tail_200_chars_without_splitting_multibyte_text() {
        let segments = vec![segment_row("短い"), segment_row(&"あ".repeat(250))];

        let prompt = initial_prompt_from_segments(&segments).unwrap();

        assert_eq!(prompt.chars().count(), 200);
        assert_eq!(prompt, "あ".repeat(200));
    }

    #[test]
    fn prompt_ignores_empty_segments() {
        let segments = vec![segment_row("  "), segment_row("確定テキスト")];

        assert_eq!(
            initial_prompt_from_segments(&segments),
            Some("確定テキスト".to_string())
        );
    }

    #[test]
    fn hallucination_filter_drops_blank_and_noise_patterns() {
        let segments = vec![
            new_segment("  "),
            new_segment("[BLANK_AUDIO]"),
            new_segment(" ご視聴ありがとうございました。 "),
            new_segment("残すテキスト"),
        ];

        let filtered = filter_hallucinated_segments(segments);

        assert_eq!(filtered, vec![new_segment("残すテキスト")]);
    }

    #[test]
    fn hallucination_filter_drops_third_and_later_identical_text() {
        let segments = vec![
            new_segment("えー"),
            new_segment(" えー "),
            new_segment("え ー"),
            new_segment("次"),
        ];

        let filtered = filter_hallucinated_segments(segments);

        assert_eq!(
            filtered,
            vec![
                new_segment("えー"),
                new_segment(" えー "),
                new_segment("次")
            ]
        );
    }

    #[test]
    fn hallucination_filter_counts_repeats_across_existing_segments() {
        let previous = vec![segment_row("えー"), segment_row("え ー")];
        let segments = vec![new_segment("えー"), new_segment("次")];

        let filtered = filter_hallucinated_segments_with_history(&previous, segments);

        assert_eq!(filtered, vec![new_segment("次")]);
    }

    #[test]
    fn thread_count_is_capped_between_one_and_eight() {
        assert_eq!(transcription_thread_count(0), 1);
        assert_eq!(transcription_thread_count(4), 4);
        assert_eq!(transcription_thread_count(64), 8);
    }

    #[test]
    fn full_params_can_be_built_for_rt_and_batch_jobs() {
        let rt_job = job(JobKind::Rt, Language::Ja);
        let batch_job = job(JobKind::Batch, Language::Auto);

        let _rt_params = build_full_params(&rt_job, Some("直前テキスト"), None);
        let _batch_params =
            build_full_params(&batch_job, None, Some(Arc::new(AtomicBool::new(false))));
    }

    #[test]
    fn processor_reads_latest_gpu_mode_from_settings_store() {
        let settings_path = temp_path("processor_reads_latest_gpu_mode", "json");
        let db_path = temp_path("processor_gpu_db", "sqlite");
        let settings_store = SettingsStore::new(settings_path.clone());
        settings_store.load().expect("default settings should load");
        let processor = WhisperJobProcessor::new(
            Arc::new(Db::open(&db_path).expect("db should open")),
            Arc::new(WhisperContextManager::new()),
            PathBuf::from("models"),
            settings_store.clone(),
            Arc::new(AtomicBool::new(false)),
        );

        assert_eq!(processor.current_gpu_mode().unwrap(), GpuMode::Auto);

        settings_store
            .update(SettingsPatch {
                gpu_mode: Some(GpuMode::ForceCpu),
                ..SettingsPatch::default()
            })
            .expect("settings should update");

        assert_eq!(processor.current_gpu_mode().unwrap(), GpuMode::ForceCpu);

        let _ = std::fs::remove_file(settings_path);
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn clipping_keeps_only_segments_owned_by_valid_range() {
        let job = TranscribeJob {
            valid_start_ms: 1_500,
            valid_end_ms: 3_000,
            ..job(JobKind::Batch, Language::Ja)
        };
        let mut before = new_segment("before");
        before.start_ms = 1_000;
        before.end_ms = 1_400;
        let mut crossing_start = new_segment("crossing-start");
        crossing_start.start_ms = 1_300;
        crossing_start.end_ms = 1_700;
        let mut crossing_end = new_segment("crossing-end");
        crossing_end.start_ms = 2_600;
        crossing_end.end_ms = 3_200;
        let mut at_end = new_segment("at-end");
        at_end.start_ms = 2_900;
        at_end.end_ms = 3_100;

        let clipped =
            clip_segments_to_valid_range(&job, vec![before, crossing_start, crossing_end, at_end]);

        assert_eq!(
            clipped,
            vec![
                NewSegment {
                    start_ms: 1_500,
                    end_ms: 1_700,
                    ..new_segment("crossing-start")
                },
                NewSegment {
                    start_ms: 2_600,
                    end_ms: 3_000,
                    ..new_segment("crossing-end")
                }
            ]
        );
    }

    #[test]
    fn similar_overlap_suppression_drops_equal_and_contained_text() {
        let previous = vec![Segment {
            text: "hello world".to_string(),
            start_ms: 1_000,
            end_ms: 2_000,
            ..segment_row("ignored")
        }];
        let mut equal_overlap = new_segment("helloworld");
        equal_overlap.start_ms = 1_900;
        equal_overlap.end_ms = 2_400;
        let mut contained_overlap = new_segment("hello world again");
        contained_overlap.start_ms = 1_950;
        contained_overlap.end_ms = 2_500;
        let mut no_overlap = new_segment("hello world");
        no_overlap.start_ms = 2_000;
        no_overlap.end_ms = 2_600;

        let filtered = suppress_similar_overlaps(
            &previous,
            vec![equal_overlap, contained_overlap, no_overlap],
        );

        assert_eq!(
            filtered,
            vec![NewSegment {
                start_ms: 2_000,
                end_ms: 2_600,
                ..new_segment("hello world")
            }]
        );
    }

    fn segment_row(text: &str) -> Segment {
        Segment {
            id: 1,
            session_id: "session-a".to_string(),
            start_ms: 0,
            end_ms: 100,
            text: text.to_string(),
            lang: Some("ja".to_string()),
        }
    }

    fn new_segment(text: &str) -> NewSegment {
        NewSegment {
            session_id: "session-a".to_string(),
            start_ms: 0,
            end_ms: 100,
            text: text.to_string(),
            lang: Some("ja".to_string()),
        }
    }

    fn job(kind: JobKind, language: Language) -> TranscribeJob {
        TranscribeJob {
            session_id: "session-a".to_string(),
            kind,
            audio: vec![0.0; 160],
            chunk_start_ms: 0,
            valid_start_ms: 0,
            valid_end_ms: 100,
            session_duration_ms: 100,
            language,
            model: "medium-q5_0".to_string(),
            canceled: Arc::new(AtomicBool::new(false)),
        }
    }

    fn temp_path(name: &str, extension: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("sokki-{name}-{nanos}.{extension}"))
    }
}
