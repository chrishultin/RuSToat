use stoat_gpui::playback::{
    Change, MediaKind, NowPlaying, UNKNOWN_TIME, UNKNOWN_VIDEO_WINDOW, clock, fraction_along,
    progress, seek_target, stops, time_label, window_size,
};

fn audio(channel: &str, file: &str) -> NowPlaying {
    NowPlaying {
        channel: channel.into(),
        file_id: file.into(),
        kind: MediaKind::Audio,
    }
}

fn video(channel: &str, file: &str) -> NowPlaying {
    NowPlaying {
        kind: MediaKind::Video,
        ..audio(channel, file)
    }
}

#[test]
fn clocks_read_minutes_and_seconds_then_hours() {
    assert_eq!(clock(0), "0:00");
    assert_eq!(clock(7), "0:07");
    assert_eq!(clock(59), "0:59");
    assert_eq!(clock(60), "1:00");
    assert_eq!(clock(195), "3:15");
    assert_eq!(clock(3599), "59:59");
    assert_eq!(clock(3600), "1:00:00");
    assert_eq!(clock(3723), "1:02:03");
}

#[test]
fn time_labels_show_position_of_duration() {
    assert_eq!(time_label(0.0, Some(195.0)), "0:00 / 3:15");
    assert_eq!(time_label(42.9, Some(195.0)), "0:42 / 3:15");
    assert_eq!(time_label(59.0, Some(59.0)), "0:59 / 0:59");
    // The length rounds; the position doesn't pass it.
    assert_eq!(time_label(194.7, Some(194.6)), "3:14 / 3:15");
    assert_eq!(time_label(400.0, Some(195.0)), "3:15 / 3:15");
    assert_eq!(time_label(62.0, Some(3723.0)), "1:02 / 1:02:03");
    assert_eq!(time_label(3723.0, Some(3723.0)), "1:02:03 / 1:02:03");
}

#[test]
fn an_unknown_length_shows_dashes() {
    let unknown = format!("{UNKNOWN_TIME} / {UNKNOWN_TIME}");
    assert_eq!(time_label(0.0, None), unknown);
    assert_eq!(time_label(12.0, None), unknown);
    assert_eq!(time_label(1.0, Some(f64::NAN)), unknown);
    assert_eq!(time_label(1.0, Some(f64::INFINITY)), unknown);
    // A junk position reads as the start.
    assert_eq!(time_label(f64::NAN, Some(10.0)), "0:00 / 0:10");
    assert_eq!(time_label(-3.0, Some(10.0)), "0:00 / 0:10");
}

#[test]
fn seek_targets_are_clamped_to_the_file() {
    assert_eq!(seek_target(0.5, 200.0), 100.0);
    assert_eq!(seek_target(0.0, 200.0), 0.0);
    assert_eq!(seek_target(1.0, 200.0), 200.0);
    assert_eq!(seek_target(-0.2, 200.0), 0.0);
    assert_eq!(seek_target(1.7, 200.0), 200.0);
    assert_eq!(seek_target(f64::NAN, 200.0), 0.0);
    assert_eq!(seek_target(0.5, f64::NAN), 0.0);
}

#[test]
fn the_bar_fills_as_it_plays() {
    assert_eq!(progress(0.0, Some(10.0)), 0.0);
    assert_eq!(progress(2.5, Some(10.0)), 0.25);
    assert_eq!(progress(12.0, Some(10.0)), 1.0);
    assert_eq!(progress(5.0, None), 0.0);
    assert_eq!(progress(5.0, Some(0.0)), 0.0);
}

#[test]
fn clicks_on_the_bar_become_fractions() {
    assert_eq!(fraction_along(150.0, 100.0, 200.0), 0.25);
    assert_eq!(fraction_along(90.0, 100.0, 200.0), 0.0);
    assert_eq!(fraction_along(400.0, 100.0, 200.0), 1.0);
    assert_eq!(fraction_along(150.0, 100.0, 0.0), 0.0);
}

#[test]
fn starting_another_file_stops_the_one_playing() {
    let playing = audio("general", "song");
    assert!(stops(&playing, Change::Play(&audio("general", "other"))));
    assert!(stops(&playing, Change::Play(&audio("music", "other"))));
    assert!(stops(&playing, Change::Play(&video("general", "clip"))));
    assert!(stops(
        &video("general", "clip"),
        Change::Play(&audio("general", "song"))
    ));
    // The same one again resumes it.
    assert!(!stops(&playing, Change::Play(&audio("general", "song"))));
}

#[test]
fn leaving_the_channel_stops_audio_but_not_a_video() {
    let playing = audio("general", "song");
    assert!(!stops(&playing, Change::OpenChannel(Some("general"))));
    assert!(stops(&playing, Change::OpenChannel(Some("music"))));
    assert!(stops(&playing, Change::OpenChannel(None)));
    let clip = video("general", "clip");
    assert!(!stops(&clip, Change::OpenChannel(Some("music"))));
    assert!(!stops(&clip, Change::OpenChannel(None)));
}

#[test]
fn switching_account_stops_anything() {
    for playing in [audio("general", "song"), video("general", "clip")] {
        assert!(stops(&playing, Change::SwitchAccount));
    }
}

#[test]
fn closing_the_window_stops_audio_but_not_a_video() {
    assert!(stops(&audio("general", "song"), Change::HideWindow));
    assert!(!stops(&video("general", "clip"), Change::HideWindow));
}

/// A 1440 × 900 laptop screen's free space: 80% is 1152 × 720.
const LAPTOP: (f64, f64) = (1440.0, 900.0);

#[test]
fn video_windows_are_the_videos_size_when_it_fits() {
    assert_eq!(window_size((640, 360), LAPTOP), (640.0, 360.0));
    assert_eq!(window_size((720, 1280), (2560.0, 1600.0)), (720.0, 1280.0));
}

#[test]
fn big_videos_shrink_to_most_of_the_screen() {
    // Landscape: width-bound.
    assert_eq!(window_size((3840, 2160), LAPTOP), (1152.0, 648.0));
    // Portrait: height-bound.
    assert_eq!(window_size((1080, 1920), LAPTOP), (405.0, 720.0));
}

#[test]
fn tiny_videos_grow_to_the_minimum() {
    assert_eq!(window_size((160, 90), LAPTOP), (320.0, 180.0));
    // Both sides reach it, keeping the shape.
    assert_eq!(window_size((90, 160), LAPTOP), (320.0, 569.0));
    // Unless the screen can't fit that: the screen wins.
    assert_eq!(window_size((10, 1000), LAPTOP), (7.0, 720.0));
}

#[test]
fn unknown_sizes_get_the_fallback_shape() {
    assert_eq!(window_size((0, 0), LAPTOP), UNKNOWN_VIDEO_WINDOW);
    assert_eq!(window_size((1920, 0), LAPTOP), UNKNOWN_VIDEO_WINDOW);
    // A tiny screen still holds it.
    assert_eq!(window_size((0, 0), (600.0, 500.0)), (480.0, 320.0));
    // No screen reported: no limit.
    assert_eq!(window_size((3840, 2160), (0.0, 0.0)), (3840.0, 2160.0));
}
