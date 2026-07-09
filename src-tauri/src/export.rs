use std::path::Path;

use serde::Deserialize;

use crate::db::{Db, Language, Segment, Session};
use crate::error::{AppError, DB_ERROR, IO_ERROR};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Txt,
    Srt,
    Md,
}

pub fn export_session(
    db: &Db,
    session_id: &str,
    format: ExportFormat,
    path: impl AsRef<Path>,
) -> Result<(), AppError> {
    let session = db
        .get_session(session_id)
        .map_err(db_error)?
        .ok_or_else(|| AppError::new(DB_ERROR, format!("session not found: {session_id}")))?;
    let segments = db.list_segments(session_id).map_err(db_error)?;
    let body = match format {
        ExportFormat::Txt => format_txt(&segments),
        ExportFormat::Srt => format_srt(&segments),
        ExportFormat::Md => format_md(&session, &segments),
    };

    std::fs::write(path, body).map_err(|error| AppError::new(IO_ERROR, error.to_string()))
}

pub fn format_txt(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_srt(segments: &[Segment]) -> String {
    let mut output = String::new();
    for (index, segment) in segments.iter().enumerate() {
        output.push_str(&(index + 1).to_string());
        output.push('\n');
        output.push_str(&format_srt_time(segment.start_ms));
        output.push_str(" --> ");
        output.push_str(&format_srt_time(segment.end_ms));
        output.push('\n');
        output.push_str(&segment.text);
        output.push_str("\n\n");
    }
    output
}

pub fn format_md(session: &Session, segments: &[Segment]) -> String {
    let mut output = String::new();
    output.push_str("# ");
    output.push_str(&session.title);
    output.push_str("\n\n");
    output.push_str("- Created: ");
    output.push_str(&format_created_at_utc(session.created_at));
    output.push('\n');
    output.push_str("- Duration: ");
    output.push_str(&format_md_duration(session.duration_ms));
    output.push('\n');
    output.push_str("- Model: ");
    output.push_str(&session.model);
    output.push('\n');
    output.push_str("- Language: ");
    output.push_str(language_code(session.language));
    output.push_str("\n\n");
    for segment in segments {
        output.push_str("**[");
        output.push_str(&format_md_timestamp(segment.start_ms));
        output.push_str("]** ");
        output.push_str(&segment.text);
        output.push('\n');
    }
    output
}

fn format_srt_time(ms: i64) -> String {
    let ms = ms.max(0);
    let millis = ms % 1_000;
    let total_seconds = ms / 1_000;
    let seconds = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let minutes = total_minutes % 60;
    let hours = total_minutes / 60;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

fn format_md_timestamp(ms: i64) -> String {
    let total_seconds = ms.max(0) / 1_000;
    let seconds = total_seconds % 60;
    let minutes = total_seconds / 60;
    format!("{minutes:02}:{seconds:02}")
}

fn format_md_duration(ms: i64) -> String {
    format_md_timestamp(ms)
}

fn format_created_at_utc(ms: i64) -> String {
    let seconds = ms.div_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_param = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_param + 2) / 5 + 1;
    let month = month_param + if month_param < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

fn language_code(language: Language) -> &'static str {
    match language {
        Language::Ja => "ja",
        Language::En => "en",
        Language::Auto => "auto",
    }
}

fn db_error(error: rusqlite::Error) -> AppError {
    AppError::new(DB_ERROR, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{SessionStatus, Source};

    #[test]
    fn formats_txt_as_segment_text_joined_with_lf() {
        let output = format_txt(&sample_segments());

        assert_eq!(output, "こんにちは\n境界テスト");
    }

    #[test]
    fn formats_srt_with_sequence_numbers_and_millisecond_timecodes() {
        let output = format_srt(&sample_segments());

        assert_eq!(
            output,
            "1\n00:00:00,000 --> 00:00:01,234\nこんにちは\n\n2\n01:01:01,001 --> 01:01:01,999\n境界テスト\n\n"
        );
    }

    #[test]
    fn formats_md_with_title_metadata_and_minute_timecodes() {
        let output = format_md(&sample_session(), &sample_segments());

        assert_eq!(
            output,
            "# 会議メモ\n\n- Created: 1970-01-15 06:56:07 UTC\n- Duration: 01:01\n- Model: medium-q5_0\n- Language: ja\n\n**[00:00]** こんにちは\n**[61:01]** 境界テスト\n"
        );
    }

    #[test]
    fn export_session_writes_selected_format_as_utf8() {
        let db = Db::open_in_memory().expect("in-memory db should migrate");
        db.insert_session(&sample_session())
            .expect("session should insert");
        for segment in sample_segments() {
            db.insert_segment(&crate::db::NewSegment {
                session_id: segment.session_id,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                text: segment.text,
                lang: segment.lang,
            })
            .expect("segment should insert");
        }
        let dir = temp_dir("export-session");
        let path = dir.join("session.srt");

        export_session(&db, "session-a", ExportFormat::Srt, &path).expect("session should export");

        let output = std::fs::read_to_string(&path).expect("export should be utf-8");
        assert_eq!(
            output,
            "1\n00:00:00,000 --> 00:00:01,234\nこんにちは\n\n2\n01:01:01,001 --> 01:01:01,999\n境界テスト\n\n"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn sample_session() -> Session {
        Session {
            id: "session-a".to_string(),
            title: "会議メモ".to_string(),
            created_at: 1_234_567_890,
            duration_ms: 61_999,
            audio_path: Some("C:/audio.wav".to_string()),
            source: Source::Import,
            language: Language::Ja,
            model: "medium-q5_0".to_string(),
            status: SessionStatus::Done,
            error_message: None,
            drop_count: 0,
        }
    }

    fn sample_segments() -> Vec<Segment> {
        vec![
            Segment {
                id: 1,
                session_id: "session-a".to_string(),
                start_ms: 0,
                end_ms: 1_234,
                text: "こんにちは".to_string(),
                lang: Some("ja".to_string()),
            },
            Segment {
                id: 2,
                session_id: "session-a".to_string(),
                start_ms: 3_661_001,
                end_ms: 3_661_999,
                text: "境界テスト".to_string(),
                lang: Some("ja".to_string()),
            },
        ]
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("sokki-export-{name}-{}", unix_time_ms()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn unix_time_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}
