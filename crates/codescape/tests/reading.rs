use codescape::canvas::FILE_OPEN_PX;
use codescape::reading::*;

#[test]
fn zooming_while_reading_scales_the_text_up_to_a_maximum() {
    assert_eq!(zoom_text(READING_OPEN_PT, 1.25), Some(16.25));
    assert_eq!(zoom_text(30.0, 2.0), Some(READING_MAX_PT));
}

#[test]
fn zooming_out_below_the_minimum_text_size_leaves_the_file() {
    assert_eq!(zoom_text(7.0, 0.9), None);
}

#[test]
fn leaving_shrinks_the_box_below_the_width_that_opens_it() {
    let f = leave_zoom(1000.0);
    assert!((1000.0 * f - 0.7 * FILE_OPEN_PX as f64).abs() < 0.01, "{f}");
}

#[test]
fn a_file_opens_small_enough_to_show_as_much_of_it_as_fits_from_the_top() {
    use codescape::source::LINE_SPACING;
    let fits = |lines: u32, view_h: f32| lines as f32 * open_font(lines, view_h) * LINE_SPACING <= view_h + 0.01;
    assert_eq!(open_font(20, 800.0), READING_OPEN_PT, "a short file keeps the usual size");
    let medium = open_font(60, 800.0);
    assert!(medium < READING_OPEN_PT && fits(60, 800.0), "{medium}");
    assert_eq!(open_font(2000, 800.0), READING_FIT_MIN_PT, "a long file stops at a readable size");
    const { assert!(READING_FIT_MIN_PT > READING_MIN_PT) };
}
