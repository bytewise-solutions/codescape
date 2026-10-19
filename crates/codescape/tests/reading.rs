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
