//! The timeline's horizontal view: which frames are visible (docs/TIMELINE.md §2).
//!
//! The view is a frame range, `start..end`, spread over the graph's width. Zooming
//! and scrolling change the range, so any length fits on screen and zooming keeps the
//! frame under the cursor in place. Everything here is pure, so it's tested directly.

/// Space before frame 0 and after the last visible frame, so keys on the edges can be
/// grabbed.
pub const PAD: f32 = 12.;
/// The fewest frames a view can show (fully zoomed in).
pub const MIN_SPAN: f32 = 4.;
/// One wheel notch or zoom button press.
pub const ZOOM_STEP: f32 = 1.25;
/// Ruler labels are at least this far apart, in points.
pub const LABEL_GAP: f32 = 44.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineView {
    /// first visible frame (fractional while scrolling)
    pub start: f32,
    /// last visible frame
    pub end: f32,
    /// the animation this view was made for; another one gets a fresh fit
    pub anim_id: i32,
    /// the range bar's length, frozen while one of its handles is dragged so the bar
    /// doesn't rescale under the pointer
    pub bar_total: Option<f32>,
}

impl Default for TimelineView {
    fn default() -> Self {
        TimelineView { start: 0., end: 0., anim_id: -1, bar_total: None }
    }
}

impl TimelineView {
    pub fn new(start: f32, end: f32) -> Self {
        TimelineView { start, end, ..Default::default() }
    }

    pub fn span(&self) -> f32 {
        self.end - self.start
    }

    pub fn is_valid(&self) -> bool {
        self.span() >= MIN_SPAN - 1e-3 && self.start.is_finite() && self.end.is_finite()
    }

    /// Points per frame across a graph `width` wide.
    pub fn px_per_frame(&self, width: f32) -> f32 {
        (width - 2. * PAD).max(1.) / self.span().max(1e-3)
    }

    /// Screen x of `frame`, in a graph spanning `left..left + width`.
    pub fn frame_to_x(&self, frame: f32, left: f32, width: f32) -> f32 {
        left + PAD + (frame - self.start) * self.px_per_frame(width)
    }

    /// The (fractional) frame at screen `x`.
    pub fn x_to_frame(&self, x: f32, left: f32, width: f32) -> f32 {
        self.start + (x - left - PAD) / self.px_per_frame(width)
    }

    /// The whole frame nearest to screen `x`, never below 0.
    pub fn nearest_frame(&self, x: f32, left: f32, width: f32) -> i32 {
        self.x_to_frame(x, left, width).round().max(0.) as i32
    }

    /// Whole frames that are (at least partly) visible.
    pub fn visible_frames(&self) -> std::ops::RangeInclusive<i32> {
        (self.start.floor().max(0.) as i32)..=(self.end.ceil().max(0.) as i32)
    }

    /// Zoom by `factor` (above 1 zooms in) keeping `anchor` (a frame) where it is on
    /// screen. `max_span` is the most frames a view may show.
    pub fn zoom_at(&mut self, anchor: f32, factor: f32, max_span: f32) {
        let span = (self.span() / factor).clamp(MIN_SPAN, max_span.max(MIN_SPAN));
        let t = if self.span() > 0. { (anchor - self.start) / self.span() } else { 0. };
        self.start = anchor - t * span;
        self.end = self.start + span;
        self.keep_in_bounds();
    }

    /// Scroll by `frames` (positive moves later in time).
    pub fn pan(&mut self, frames: f32) {
        self.start += frames;
        self.end += frames;
        self.keep_in_bounds();
    }

    /// Show `first..=last`, with a little room on either side.
    pub fn fit(&mut self, first: f32, last: f32, max_span: f32) {
        let (first, last) = (first.min(last), first.max(last));
        let span = ((last - first) * 1.1 + 2.).clamp(MIN_SPAN, max_span.max(MIN_SPAN));
        let mid = (first + last) / 2.;
        self.start = mid - span / 2.;
        self.end = self.start + span;
        self.keep_in_bounds();
    }

    /// The view never starts before frame 0.
    fn keep_in_bounds(&mut self) {
        if self.start < 0. {
            self.end -= self.start;
            self.start = 0.;
        }
    }
}

/// How many frames the timeline holds for an animation whose last key is on
/// `last_key` (`None` without keys): the keys plus room to add more (2 s), and at
/// least 5 s. The range bar spans this, and it's the furthest the view zooms out.
pub fn content_frames(last_key: Option<i32>, fps: i32) -> f32 {
    let fps = fps.max(1) as f32;
    let last = last_key.unwrap_or(0).max(0) as f32;
    (last + fps * 2.).max(fps * 5.)
}

/// The frames the view covers when an animation is opened: the whole animation up to
/// its last key, plus a little.
pub fn initial_view(last_key: Option<i32>, fps: i32) -> TimelineView {
    let total = content_frames(last_key, fps);
    let mut view = TimelineView::new(0., 0.);
    match last_key {
        Some(last) if last > 0 => view.fit(0., last as f32 + fps.max(1) as f32 * 0.5, total),
        _ => view.fit(0., fps.max(1) as f32 * 2., total),
    }
    view
}

/// What a press on the range bar grabbed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BarHandle {
    /// the thumb's middle: scroll
    Middle,
    /// its left end: move the view's start (zoom)
    Start,
    /// its right end: move the view's end (zoom)
    End,
}

/// The range bar's handle ends are this wide, in points.
pub const BAR_HANDLE: f32 = 9.;
/// The thumb is never drawn narrower than this, so its handles stay grabbable.
pub const BAR_MIN_THUMB: f32 = 3. * BAR_HANDLE;

/// The thumb's screen span on a bar spanning `left..left + width` that shows frames
/// `0..total`. A thumb narrower than `BAR_MIN_THUMB` grows about its centre.
pub fn bar_thumb(view: &TimelineView, total: f32, left: f32, width: f32) -> (f32, f32) {
    let total = total.max(view.end).max(1.);
    let a = left + view.start / total * width;
    let b = left + view.end / total * width;
    if b - a >= BAR_MIN_THUMB {
        return (a, b);
    }
    let mid = ((a + b) / 2.).clamp(left + BAR_MIN_THUMB / 2., left + width - BAR_MIN_THUMB / 2.);
    (mid - BAR_MIN_THUMB / 2., mid + BAR_MIN_THUMB / 2.)
}

/// Which part of the thumb screen `x` is on, if any.
pub fn bar_hit(view: &TimelineView, total: f32, left: f32, width: f32, x: f32) -> Option<BarHandle> {
    let (a, b) = bar_thumb(view, total, left, width);
    if x < a || x > b {
        None
    } else if x <= a + BAR_HANDLE {
        Some(BarHandle::Start)
    } else if x >= b - BAR_HANDLE {
        Some(BarHandle::End)
    } else {
        Some(BarHandle::Middle)
    }
}

/// Drag the range bar's `handle` by `frames` (the pointer's movement in bar frames).
/// The view stays within `0..total` and at least `MIN_SPAN` wide.
pub fn bar_drag(view: &mut TimelineView, handle: BarHandle, frames: f32, total: f32) {
    let total = total.max(view.end);
    match handle {
        BarHandle::Middle => {
            let d = frames.clamp(-view.start, total - view.end);
            view.start += d;
            view.end += d;
        }
        BarHandle::Start => view.start = (view.start + frames).clamp(0., view.end - MIN_SPAN),
        BarHandle::End => view.end = (view.end + frames).clamp(view.start + MIN_SPAN, total),
    }
}

/// The step between ruler labels, in frames, so labels are at least `LABEL_GAP`
/// apart. In frames: 1, 2, 5, 10, 20, 50… In seconds: whole fractions of a second
/// (frame counts that divide `fps`), then 1, 2, 5, 10… seconds.
pub fn ruler_step(px_per_frame: f32, fps: i32, seconds: bool) -> i32 {
    let fits = |step: i32| step as f32 * px_per_frame >= LABEL_GAP;
    let fps = fps.max(1);
    if seconds {
        if let Some(d) = (1..fps).filter(|d| fps % d == 0).find(|d| fits(*d)) {
            return d;
        }
        return nice_steps().map(|s| s * fps).find(|s| fits(*s)).unwrap_or(i32::MAX / 2);
    }
    nice_steps().find(|s| fits(*s)).unwrap_or(i32::MAX / 2)
}

/// The step between drawn frame lines: every frame while they're at least 6 points
/// apart, then every 2nd, 5th, 10th… frame.
pub fn line_step(px_per_frame: f32) -> i32 {
    nice_steps().find(|s| *s as f32 * px_per_frame >= 6.).unwrap_or(i32::MAX / 2)
}

/// 1, 2, 5, 10, 20, 50, 100…
fn nice_steps() -> impl Iterator<Item = i32> {
    (0..9).flat_map(|p| [1, 2, 5].map(|m| m * 10_i32.pow(p)))
}

/// A ruler label: the frame number, or seconds ("1s", "0.5s", "1.25s").
pub fn ruler_label(frame: i32, fps: i32, seconds: bool) -> String {
    if !seconds {
        return frame.to_string();
    }
    let s = frame as f32 / fps.max(1) as f32;
    let text = format!("{:.2}", s);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}s")
}
