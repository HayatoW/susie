use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use metrics::{
    append_sample, read_samples, ClockError, Error, FrameClock, LineError, Progress, Sample,
};

#[test]
fn given_numbers_round_trip_as_one_line() {
    let path = temp_path("metrics.jsonl");
    let sample = Sample {
        startup_ms: 128,
        fps: 60,
    };

    append_sample(&path, sample).unwrap();

    let body = fs::read_to_string(&path).unwrap();
    assert_eq!(body, "{\"startup_ms\":128,\"fps\":60}\n");
    assert_eq!(read_samples(&path).unwrap(), vec![sample]);
}

#[test]
fn append_keeps_the_earlier_line() {
    let path = temp_path("metrics.jsonl");
    let first = Sample {
        startup_ms: 128,
        fps: 60,
    };
    let second = Sample {
        startup_ms: 7,
        fps: 1,
    };

    append_sample(&path, first).unwrap();
    append_sample(&path, second).unwrap();

    let body = fs::read_to_string(&path).unwrap();
    assert_eq!(
        body,
        "{\"startup_ms\":128,\"fps\":60}\n{\"startup_ms\":7,\"fps\":1}\n"
    );
    assert_eq!(read_samples(&path).unwrap(), vec![first, second]);
}

#[test]
fn missing_file_is_an_error() {
    let path = temp_path("missing.jsonl");

    match read_samples(&path) {
        Err(Error::Io(source)) => assert_eq!(source.kind(), std::io::ErrorKind::NotFound),
        other => panic!("missing file should be NotFound, got {other:?}"),
    }
}

#[test]
fn empty_file_has_no_samples() {
    let path = temp_path("empty.jsonl");
    fs::write(&path, "").unwrap();

    assert_eq!(read_samples(&path).unwrap(), Vec::<Sample>::new());
}

#[test]
fn reader_rejects_a_fractional_fps() {
    let path = temp_path("fraction.jsonl");
    fs::write(&path, "{\"startup_ms\":1,\"fps\":1.5}\n").unwrap();

    assert!(matches!(
        read_samples(&path),
        Err(Error::Line {
            number: 1,
            reason: LineError::Json(_),
        })
    ));
}

#[test]
fn reader_rejects_a_negative_startup() {
    let path = temp_path("negative.jsonl");
    fs::write(&path, "{\"startup_ms\":-1,\"fps\":1}\n").unwrap();

    assert!(matches!(
        read_samples(&path),
        Err(Error::Line {
            number: 1,
            reason: LineError::Json(_),
        })
    ));
}

#[test]
fn reader_rejects_a_missing_fps() {
    let path = temp_path("missing-field.jsonl");
    fs::write(&path, "{\"startup_ms\":1}\n").unwrap();

    assert!(matches!(
        read_samples(&path),
        Err(Error::Line {
            number: 1,
            reason: LineError::Json(_),
        })
    ));
}

#[test]
fn reader_rejects_a_string_number() {
    let path = temp_path("string.jsonl");
    fs::write(&path, "{\"startup_ms\":\"128\",\"fps\":60}\n").unwrap();

    assert!(matches!(
        read_samples(&path),
        Err(Error::Line {
            number: 1,
            reason: LineError::Json(_),
        })
    ));
}

#[test]
fn reader_rejects_an_fps_above_u32() {
    let path = temp_path("wide.jsonl");
    fs::write(&path, "{\"startup_ms\":1,\"fps\":4294967296}\n").unwrap();

    assert!(matches!(
        read_samples(&path),
        Err(Error::Line {
            number: 1,
            reason: LineError::Json(_),
        })
    ));
}

#[test]
fn reader_keeps_numbers_when_an_extra_field_is_present() {
    let path = temp_path("extra.jsonl");
    fs::write(&path, "{\"fps\":2,\"startup_ms\":1,\"host\":\"vm\"}\n").unwrap();

    assert_eq!(
        read_samples(&path).unwrap(),
        vec![Sample {
            startup_ms: 1,
            fps: 2
        }]
    );
}

#[test]
fn blank_line_is_an_error() {
    let path = temp_path("blank.jsonl");
    fs::write(
        &path,
        "{\"startup_ms\":1,\"fps\":2}\n\n{\"startup_ms\":3,\"fps\":4}\n",
    )
    .unwrap();

    assert!(matches!(
        read_samples(&path),
        Err(Error::Line {
            number: 2,
            reason: LineError::Empty,
        })
    ));
}

#[test]
fn a_bad_second_line_does_not_return_the_first_sample() {
    let path = temp_path("partial.jsonl");
    fs::write(&path, "{\"startup_ms\":1,\"fps\":2}\nnot-json\n").unwrap();

    assert!(matches!(
        read_samples(&path),
        Err(Error::Line {
            number: 2,
            reason: LineError::Json(_),
        })
    ));
}

#[test]
fn crlf_line_reads_the_same_numbers() {
    let path = temp_path("crlf.jsonl");
    fs::write(&path, "{\"startup_ms\":3,\"fps\":4}\r\n").unwrap();

    assert_eq!(
        read_samples(&path).unwrap(),
        vec![Sample {
            startup_ms: 3,
            fps: 4
        }]
    );
}

#[test]
fn append_separates_a_record_that_has_no_trailing_newline() {
    let path = temp_path("nonewline-append.jsonl");
    fs::write(&path, "{\"startup_ms\":4,\"fps\":5}").unwrap();

    append_sample(
        &path,
        Sample {
            startup_ms: 7,
            fps: 8,
        },
    )
    .unwrap();

    let body = fs::read_to_string(&path).unwrap();
    assert_eq!(
        body,
        "{\"startup_ms\":4,\"fps\":5}\n{\"startup_ms\":7,\"fps\":8}\n"
    );
    assert_eq!(
        read_samples(&path).unwrap(),
        vec![
            Sample {
                startup_ms: 4,
                fps: 5
            },
            Sample {
                startup_ms: 7,
                fps: 8
            },
        ]
    );
}

#[test]
fn one_record_without_a_trailing_newline_still_reads() {
    let path = temp_path("nonewline.jsonl");
    fs::write(&path, "{\"startup_ms\":4,\"fps\":5}").unwrap();

    assert_eq!(
        read_samples(&path).unwrap(),
        vec![Sample {
            startup_ms: 4,
            fps: 5
        }]
    );
}

#[test]
fn append_creates_a_missing_parent_directory() {
    let path = temp_path("nested").join("metrics.jsonl");

    append_sample(
        &path,
        Sample {
            startup_ms: 9,
            fps: 8,
        },
    )
    .unwrap();

    assert_eq!(
        read_samples(&path).unwrap(),
        vec![Sample {
            startup_ms: 9,
            fps: 8
        }]
    );
}

#[test]
fn evidence_path_is_the_board_path() {
    assert_eq!(
        metrics::EVIDENCE_RELATIVE_PATH,
        "docs/evidence/metrics.jsonl"
    );
}

#[test]
fn extreme_integers_round_trip() {
    let path = temp_path("extreme.jsonl");
    let sample = Sample {
        startup_ms: u64::MAX,
        fps: u32::MAX,
    };

    append_sample(&path, sample).unwrap();

    assert_eq!(read_samples(&path).unwrap(), vec![sample]);
}

#[test]
fn clock_counts_frames_from_the_first_until_one_second() {
    let origin = Instant::now();
    let first = origin + Duration::from_millis(12);
    let mut clock = FrameClock::waiting(origin);

    let started = clock.record_frame(first).unwrap();
    assert_eq!(
        started,
        Progress::Counting {
            startup_ms: 12,
            frames: 1
        }
    );

    let mut offset_ms = 16u64;
    while offset_ms < 1_000 {
        let progress = clock
            .record_frame(first + Duration::from_millis(offset_ms))
            .unwrap();
        assert!(matches!(
            progress,
            Progress::Counting { startup_ms: 12, .. }
        ));
        offset_ms += 16;
    }

    let sample = clock.finish(first + Duration::from_secs(1)).unwrap();
    assert_eq!(
        sample,
        Sample {
            startup_ms: 12,
            fps: 63
        }
    );
}

#[test]
fn frame_at_exactly_one_second_closes_without_counting_it() {
    let origin = Instant::now();
    let mut clock = FrameClock::waiting(origin);

    clock.record_frame(origin).unwrap();
    let progress = clock.record_frame(origin + Duration::from_secs(1)).unwrap();

    assert_eq!(
        progress,
        Progress::Done(Sample {
            startup_ms: 0,
            fps: 1
        })
    );
    assert_eq!(
        clock
            .record_frame(origin + Duration::from_millis(1_500))
            .unwrap(),
        Progress::Done(Sample {
            startup_ms: 0,
            fps: 1
        })
    );
}

#[test]
fn sub_millisecond_startup_truncates_toward_zero() {
    let origin = Instant::now();

    let mut short = FrameClock::waiting(origin);
    let progress = short
        .record_frame(origin + Duration::from_micros(999))
        .unwrap();
    assert_eq!(
        progress,
        Progress::Counting {
            startup_ms: 0,
            frames: 1
        }
    );

    let mut longer = FrameClock::waiting(origin);
    let progress = longer
        .record_frame(origin + Duration::from_micros(1_999))
        .unwrap();
    assert_eq!(
        progress,
        Progress::Counting {
            startup_ms: 1,
            frames: 1
        }
    );
}

#[test]
fn frame_before_origin_keeps_the_clock_waiting() {
    let origin = Instant::now();
    let mut clock = FrameClock::waiting(origin);

    let error = clock
        .record_frame(origin - Duration::from_millis(1))
        .unwrap_err();
    assert_eq!(error, ClockError::FrameBeforeOrigin);

    let progress = clock
        .record_frame(origin + Duration::from_millis(5))
        .unwrap();
    assert_eq!(
        progress,
        Progress::Counting {
            startup_ms: 5,
            frames: 1
        }
    );
}

#[test]
fn early_finish_does_not_close_and_a_later_finish_can_repeat() {
    let origin = Instant::now();
    let mut clock = FrameClock::waiting(origin);
    let error = clock.finish(origin + Duration::from_secs(2)).unwrap_err();
    assert_eq!(error, ClockError::NotStarted);

    clock.record_frame(origin).unwrap();
    let error = clock
        .finish(origin + Duration::from_millis(999))
        .unwrap_err();
    assert_eq!(error, ClockError::WindowStillOpen);

    let sample = clock.finish(origin + Duration::from_secs(1)).unwrap();
    assert_eq!(
        sample,
        Sample {
            startup_ms: 0,
            fps: 1
        }
    );
    assert_eq!(
        clock.finish(origin + Duration::from_secs(3)).unwrap(),
        sample
    );
}

#[test]
fn out_of_order_frame_does_not_change_the_count() {
    let origin = Instant::now();
    let mut clock = FrameClock::waiting(origin);
    clock
        .record_frame(origin + Duration::from_millis(10))
        .unwrap();
    clock
        .record_frame(origin + Duration::from_millis(20))
        .unwrap();

    let error = clock
        .record_frame(origin + Duration::from_millis(15))
        .unwrap_err();
    assert_eq!(error, ClockError::FrameOutOfOrder);

    let sample = clock.finish(origin + Duration::from_millis(10) + Duration::from_secs(1));
    assert_eq!(
        sample.unwrap(),
        Sample {
            startup_ms: 10,
            fps: 2
        }
    );
}

#[test]
fn recorded_sample_round_trips_through_the_log() {
    let origin = Instant::now();
    let first = origin + Duration::from_millis(12);
    let mut clock = FrameClock::waiting(origin);
    clock.record_frame(first).unwrap();
    clock
        .record_frame(first + Duration::from_millis(500))
        .unwrap();
    let sample = clock.finish(first + Duration::from_secs(1)).unwrap();
    assert_eq!(
        sample,
        Sample {
            startup_ms: 12,
            fps: 2
        }
    );

    let path = temp_path("recorded.jsonl");
    append_sample(&path, sample).unwrap();
    assert_eq!(read_samples(&path).unwrap(), vec![sample]);
}

fn temp_path(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("susie-metrics-{id}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}
