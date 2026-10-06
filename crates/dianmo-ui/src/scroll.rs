//! One-axis scrolling with a short momentum fling, driven by `View::timer` frames.

/// Animation frame interval while flinging.
pub(crate) const FRAME_MS: u64 = 16;

#[derive(Clone, Debug, Default)]
pub(crate) struct Scroller {
    /// Content offset, 0..=max.
    pub offset: f32,
    pub max: f32,
    /// Fling velocity in offset DIPs per ms; 0 when at rest.
    vel: f32,
    last_ms: u64,
    /// Offset when the current drag started.
    drag_origin: f32,
}

impl Scroller {
    pub fn reset(&mut self) {
        *self = Self { max: self.max, ..Self::default() };
    }

    pub fn set_max(&mut self, max: f32) {
        self.max = max.max(0.0);
        self.offset = self.offset.clamp(0.0, self.max);
    }

    pub fn animating(&self) -> bool {
        self.vel != 0.0
    }

    /// Stops a fling. Returns whether one was running.
    pub fn stop(&mut self) -> bool {
        let was = self.animating();
        self.vel = 0.0;
        was
    }

    pub fn begin_drag(&mut self) {
        self.vel = 0.0;
        self.drag_origin = self.offset;
    }

    /// `delta`: finger movement since the drag began (positive = right/down).
    pub fn drag_to(&mut self, delta: f32) -> bool {
        let old = self.offset;
        self.offset = (self.drag_origin - delta).clamp(0.0, self.max);
        old != self.offset
    }

    /// `finger_vel`: finger velocity at release, DIPs per ms (positive = right/down).
    pub fn fling(&mut self, finger_vel: f32, now: u64) {
        let v = (-finger_vel).clamp(-6.0, 6.0);
        if v.abs() > 0.15 && self.max > 0.0 {
            self.vel = v;
            self.last_ms = now;
        }
    }

    /// Advances a fling to `now`. Returns whether the offset changed.
    pub fn step(&mut self, now: u64) -> bool {
        if !self.animating() {
            return false;
        }
        let dt = now.saturating_sub(self.last_ms).min(64) as f32;
        self.last_ms = now;
        let old = self.offset;
        self.offset = (self.offset + self.vel * dt).clamp(0.0, self.max);
        self.vel *= 0.93f32.powf(dt / FRAME_MS as f32);
        if self.vel.abs() < 0.03 || self.offset <= 0.0 || self.offset >= self.max {
            self.vel = 0.0;
        }
        old != self.offset
    }
}

/// Recent finger positions along one axis, for the release velocity.
#[derive(Clone, Debug, Default)]
pub(crate) struct VelocityTracker {
    samples: Vec<(f32, u64)>,
}

impl VelocityTracker {
    pub fn push(&mut self, pos: f32, t: u64) {
        self.samples.push((pos, t));
        self.samples.retain(|&(_, st)| t.saturating_sub(st) <= 100);
    }

    /// DIPs per ms over the last ~100 ms.
    pub fn velocity(&self, now: u64) -> f32 {
        let recent: Vec<_> = self.samples.iter().filter(|&&(_, t)| now.saturating_sub(t) <= 100).collect();
        match (recent.first(), recent.last()) {
            (Some(&&(p0, t0)), Some(&&(p1, t1))) if t1 > t0 => (p1 - p0) / (t1 - t0) as f32,
            _ => 0.0,
        }
    }
}
