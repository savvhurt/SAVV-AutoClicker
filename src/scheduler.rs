use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::backend::Backend;
use crate::state::{ClickMode, Settings, SharedState, Snapshot, StopReason};

const MAX_SLEEP_SLICE: Duration = Duration::from_millis(20);
const MIN_PERIOD: Duration = Duration::from_millis(1);
const MAX_PERIOD: Duration = Duration::from_secs(3600);
const MIN_HOLD: Duration = Duration::from_micros(100);
const RELEASE_GAP: Duration = Duration::from_micros(200);
const MULTI_CLICK_GAP: Duration = Duration::from_millis(40);

#[cfg(target_os = "linux")]
const SPIN_MARGIN: Duration = Duration::from_micros(200);

#[cfg(windows)]
const SPIN_MARGIN: Duration = Duration::from_millis(1);

#[cfg(not(any(target_os = "linux", windows)))]
const SPIN_MARGIN: Duration = Duration::from_millis(1);

pub fn run(shared: Arc<SharedState>, mut backend: Box<dyn Backend>) {
    let mut rng = Rng::new();

    loop {
        let snapshot = shared.snapshot();
        if snapshot.quit {
            break;
        }
        if !snapshot.enabled {
            shared.wait_for_change(snapshot.version);
            continue;
        }
        click_series(&shared, backend.as_mut(), snapshot, &mut rng);
    }
}

fn click_series(shared: &SharedState, backend: &mut dyn Backend, initial: Snapshot, rng: &mut Rng) {
    if !shared.wait_for_start(initial.version) {
        return;
    }

    let mut deadline = Instant::now();
    let mut sequence_index = 0usize;

    loop {
        let snapshot = shared.snapshot();
        if !snapshot.enabled || snapshot.quit || snapshot.version != initial.version {
            break;
        }

        let settings = snapshot.settings;
        let period = jittered_period(settings, rng);
        let hold = clamp_hold(settings.hold, period);

        if !sleep_until(deadline, shared, initial.version) {
            break;
        }

        let button = settings.button;

        let target = match settings.mode {
            ClickMode::Cursor => None,
            ClickMode::Point => settings.target,
            ClickMode::Sequence => {
                let count = settings.point_count();
                if count == 0 {
                    shared.set_error("sequence mode has no points".to_string());
                    shared.off();
                    break;
                }
                let point = settings.point_at(sequence_index % count);
                sequence_index = (sequence_index + 1) % count;
                point
            }
        };

        if let Some(target) = target {
            if !backend.supports_target() {
                shared.set_error(
                    "clicking at a fixed point is not supported on this platform".to_string(),
                );
                shared.off();
                break;
            }

            if let Err(error) = backend.move_to(target) {
                shared.set_error(format!("cursor move failed: {error:#}"));
                shared.off();
                break;
            }
        }

        let mut interrupted = false;
        let mut limit_reached = false;

        for index in 0..settings.multi_click.max(1) {
            if let Err(error) = backend.press(button) {
                shared.set_error(format!("press failed: {error:#}"));
                shared.off();
                interrupted = true;
                break;
            }

            // Even when the stop flag interrupts the hold, the button is
            // released immediately afterwards.
            let completed = sleep_until(Instant::now() + hold, shared, initial.version);
            if let Err(error) = backend.release(button) {
                shared.set_error(format!("release failed: {error:#}"));
                shared.off();
                interrupted = true;
                break;
            }

            let reached = shared.record_click();
            if !completed {
                interrupted = true;
                break;
            }
            if reached {
                limit_reached = true;
                break;
            }

            if index + 1 < settings.multi_click
                && !sleep_until(Instant::now() + MULTI_CLICK_GAP, shared, initial.version)
            {
                interrupted = true;
                break;
            }
        }

        if limit_reached {
            shared.stop(StopReason::Limit);
            break;
        }
        if interrupted {
            break;
        }

        deadline += period;
        if deadline <= Instant::now() {
            deadline = Instant::now();
        }
    }
}

fn jittered_period(settings: Settings, rng: &mut Rng) -> Duration {
    let base = match settings.mode {
        ClickMode::Sequence => settings.point_interval,
        ClickMode::Cursor | ClickMode::Point => Duration::from_secs_f64(1.0 / settings.cps),
    };
    jittered_interval(base, settings.jitter_pct, rng)
}

fn jittered_interval(base: Duration, jitter_pct: f64, rng: &mut Rng) -> Duration {
    let jitter = if jitter_pct > 0.0 {
        (rng.next_f64() * 2.0 - 1.0) * jitter_pct / 100.0
    } else {
        0.0
    };

    // A 100% jitter may not collapse the interval to zero and turn the
    // clicker into a burst generator.
    let factor = (1.0 + jitter).max(0.1);
    Duration::from_secs_f64(base.as_secs_f64() * factor).clamp(MIN_PERIOD, MAX_PERIOD)
}

fn clamp_hold(requested: Duration, period: Duration) -> Duration {
    let max_hold = period.saturating_sub(RELEASE_GAP);
    requested
        .min(max_hold)
        .max(MIN_HOLD)
        .min(period.max(MIN_HOLD))
}

fn sleep_until(deadline: Instant, shared: &SharedState, version: u64) -> bool {
    loop {
        if shared.should_stop(version) {
            return false;
        }

        let now = Instant::now();
        if now >= deadline {
            return true;
        }

        let remaining = deadline - now;
        if remaining > SPIN_MARGIN {
            let wait = (remaining - SPIN_MARGIN).min(MAX_SLEEP_SLICE);
            if shared.wait_for_stop(version, wait) {
                return false;
            }
        } else {
            std::hint::spin_loop();
        }
    }
}

struct Rng(u64);

impl Rng {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0);
        let seed = nanos ^ (std::process::id() as u64).rotate_left(32) ^ 0x9E37_79B9_7F4A_7C15;
        Self(if seed == 0 { 1 } else { seed })
    }

    fn next_u64(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.0 = value;
        value.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use super::*;
    use crate::state::{Button, Point};

    struct MockBackend {
        presses: Arc<AtomicUsize>,
        releases: Arc<AtomicUsize>,
        moves: Arc<Mutex<Vec<Point>>>,
        down: bool,
    }

    impl MockBackend {
        #[allow(clippy::type_complexity)]
        fn new() -> (
            Self,
            Arc<AtomicUsize>,
            Arc<AtomicUsize>,
            Arc<Mutex<Vec<Point>>>,
        ) {
            let presses = Arc::new(AtomicUsize::new(0));
            let releases = Arc::new(AtomicUsize::new(0));
            let moves = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    presses: presses.clone(),
                    releases: releases.clone(),
                    moves: moves.clone(),
                    down: false,
                },
                presses,
                releases,
                moves,
            )
        }
    }

    impl Backend for MockBackend {
        fn press(&mut self, _button: Button) -> anyhow::Result<()> {
            assert!(!self.down, "press while the button is already down");
            self.down = true;
            self.presses.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn release(&mut self, _button: Button) -> anyhow::Result<()> {
            assert!(self.down, "release without a matching press");
            self.down = false;
            self.releases.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn move_to(&mut self, point: Point) -> anyhow::Result<()> {
            self.moves.lock().unwrap().push(point);
            Ok(())
        }

        fn supports_target(&self) -> bool {
            true
        }
    }

    #[allow(clippy::type_complexity)]
    fn spawn_clicker(
        settings: Settings,
    ) -> (
        Arc<SharedState>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
        Arc<Mutex<Vec<Point>>>,
        thread::JoinHandle<()>,
    ) {
        let shared = Arc::new(SharedState::new(settings, true));
        let (backend, presses, releases, moves) = MockBackend::new();
        let thread_shared = shared.clone();
        let handle = thread::spawn(move || run(thread_shared, Box::new(backend)));
        (shared, presses, releases, moves, handle)
    }

    #[test]
    fn limit_stops_after_exactly_the_requested_number_of_clicks() {
        let settings = Settings::new(1000.0, Button::Left, 0.2, 0.0)
            .unwrap()
            .with_limit(5)
            .unwrap();
        let (shared, presses, releases, _moves, handle) = spawn_clicker(settings);

        let deadline = Instant::now() + Duration::from_secs(2);
        while shared.status().enabled && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        let status = shared.status();
        assert!(!status.enabled, "limit should stop the session");
        assert_eq!(status.stop_reason, Some(StopReason::Limit));
        assert_eq!(presses.load(Ordering::SeqCst), 5);
        assert_eq!(
            presses.load(Ordering::SeqCst),
            releases.load(Ordering::SeqCst)
        );

        shared.quit();
        handle.join().unwrap();
    }

    #[test]
    fn delay_waits_before_the_first_click() {
        let settings = Settings::new(1000.0, Button::Left, 0.2, 0.0)
            .unwrap()
            .with_limit(1)
            .unwrap()
            .with_delay_ms(150.0)
            .unwrap();
        let (shared, presses, releases, _moves, handle) = spawn_clicker(settings);
        let started = Instant::now();

        thread::sleep(Duration::from_millis(50));
        assert_eq!(presses.load(Ordering::SeqCst), 0);

        let deadline = Instant::now() + Duration::from_secs(2);
        while presses.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }

        // The release happens after the configured hold, so wait for the
        // worker to finish the click instead of racing it on a loaded runner.
        let release_deadline = Instant::now() + Duration::from_secs(2);
        while releases.load(Ordering::SeqCst) == 0 && Instant::now() < release_deadline {
            thread::sleep(Duration::from_millis(2));
        }

        assert!(started.elapsed() >= Duration::from_millis(130));
        assert_eq!(presses.load(Ordering::SeqCst), 1);
        assert_eq!(
            presses.load(Ordering::SeqCst),
            releases.load(Ordering::SeqCst)
        );

        shared.quit();
        handle.join().unwrap();
    }

    #[test]
    fn stopping_during_the_start_delay_cancels_the_session() {
        let settings = Settings::default().with_delay_ms(300.0).unwrap();
        let (shared, presses, releases, _moves, handle) = spawn_clicker(settings);

        thread::sleep(Duration::from_millis(50));
        shared.off();
        thread::sleep(Duration::from_millis(400));

        assert_eq!(presses.load(Ordering::SeqCst), 0);
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        assert!(!shared.status().enabled);
        assert_eq!(shared.status().stop_reason, Some(StopReason::Manual));

        shared.quit();
        handle.join().unwrap();
    }

    #[test]
    fn hold_never_exceeds_the_period() {
        let period = Duration::from_millis(1);
        let hold = clamp_hold(Duration::from_millis(10), period);
        assert!(hold < period);
        assert!(hold >= MIN_HOLD);
    }

    #[test]
    fn jitter_stays_inside_the_requested_band() {
        let settings = Settings::new(50.0, Button::Left, 10.0, 25.0).unwrap();
        let mut rng = Rng::new();
        let nominal = 1.0 / settings.cps;
        for _ in 0..10_000 {
            let period = jittered_period(settings, &mut rng).as_secs_f64();
            assert!(period >= nominal * 0.75 - 1e-9);
            assert!(period <= nominal * 1.25 + 1e-9);
        }
    }

    #[test]
    fn extreme_jitter_does_not_collapse_the_period() {
        let settings = Settings::new(50.0, Button::Left, 10.0, 100.0).unwrap();
        let mut rng = Rng::new();
        let nominal = 1.0 / settings.cps;
        for _ in 0..10_000 {
            let period = jittered_period(settings, &mut rng).as_secs_f64();
            assert!(period >= nominal * 0.1 - 1e-9);
            assert!(period <= nominal * 2.0 + 1e-9);
        }
    }

    #[test]
    fn presses_and_releases_are_paired() {
        let settings = Settings::new(200.0, Button::Left, 1.0, 0.0).unwrap();
        let (shared, presses, releases, _moves, handle) = spawn_clicker(settings);

        thread::sleep(Duration::from_millis(150));
        shared.quit();
        handle.join().unwrap();

        let presses = presses.load(Ordering::SeqCst);
        let releases = releases.load(Ordering::SeqCst);
        assert!(presses >= 10, "expected several clicks, got {presses}");
        assert_eq!(presses, releases);
    }

    #[test]
    fn stopping_during_a_hold_releases_the_button() {
        let settings = Settings::new(1.0, Button::Left, 400.0, 0.0).unwrap();
        let (shared, presses, releases, _moves, handle) = spawn_clicker(settings);

        let deadline = Instant::now() + Duration::from_secs(2);
        while presses.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(presses.load(Ordering::SeqCst) >= 1);

        thread::sleep(Duration::from_millis(20));
        let stopped_at = Instant::now();
        shared.off();

        let release_deadline = Instant::now() + Duration::from_secs(1);
        while releases.load(Ordering::SeqCst) == 0 && Instant::now() < release_deadline {
            thread::sleep(Duration::from_millis(2));
        }

        let presses = presses.load(Ordering::SeqCst);
        let releases = releases.load(Ordering::SeqCst);
        assert_eq!(presses, releases);
        assert!(
            stopped_at.elapsed() < Duration::from_millis(250),
            "release took {:?}",
            stopped_at.elapsed()
        );

        shared.quit();
        handle.join().unwrap();
    }

    #[test]
    fn multi_click_sends_a_series_and_counts_each_click() {
        let settings = Settings::new(50.0, Button::Left, 1.0, 0.0)
            .unwrap()
            .with_multi_click(3)
            .unwrap()
            .with_limit(3)
            .unwrap();
        let (shared, presses, releases, _moves, handle) = spawn_clicker(settings);

        let deadline = Instant::now() + Duration::from_secs(2);
        while shared.status().enabled && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        let status = shared.status();
        assert!(!status.enabled);
        assert_eq!(status.stop_reason, Some(StopReason::Limit));
        assert_eq!(presses.load(Ordering::SeqCst), 3);
        assert_eq!(releases.load(Ordering::SeqCst), 3);

        shared.quit();
        handle.join().unwrap();
    }

    #[test]
    fn multi_click_stops_mid_burst_when_the_limit_is_reached() {
        let settings = Settings::new(50.0, Button::Left, 1.0, 0.0)
            .unwrap()
            .with_multi_click(3)
            .unwrap()
            .with_limit(2)
            .unwrap();
        let (shared, presses, releases, _moves, handle) = spawn_clicker(settings);

        let deadline = Instant::now() + Duration::from_secs(2);
        while shared.status().enabled && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(presses.load(Ordering::SeqCst), 2);
        assert_eq!(releases.load(Ordering::SeqCst), 2);
        assert_eq!(shared.status().stop_reason, Some(StopReason::Limit));

        shared.quit();
        handle.join().unwrap();
    }

    #[test]
    fn target_is_moved_before_the_burst() {
        let point = Point::new(10, -20);
        let settings = Settings::new(50.0, Button::Left, 1.0, 0.0)
            .unwrap()
            .with_mode(ClickMode::Point)
            .with_target(Some(point))
            .unwrap()
            .with_limit(1)
            .unwrap();
        let (shared, presses, _releases, moves, handle) = spawn_clicker(settings);

        let deadline = Instant::now() + Duration::from_secs(2);
        while shared.status().enabled && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(presses.load(Ordering::SeqCst), 1);
        assert_eq!(*moves.lock().unwrap(), vec![point]);

        shared.quit();
        handle.join().unwrap();
    }

    #[test]
    fn sequence_cycles_through_points_in_order() {
        let points = [Point::new(10, 20), Point::new(30, 40), Point::new(-5, -6)];
        let settings = Settings::new(50.0, Button::Left, 0.2, 0.0)
            .unwrap()
            .with_mode(ClickMode::Sequence)
            .with_points(&points)
            .unwrap()
            .with_point_interval_ms(20)
            .unwrap()
            .with_limit(6)
            .unwrap();
        let (shared, presses, releases, moves, handle) = spawn_clicker(settings);

        let deadline = Instant::now() + Duration::from_secs(3);
        while shared.status().enabled && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        let status = shared.status();
        assert_eq!(status.stop_reason, Some(StopReason::Limit));
        assert_eq!(presses.load(Ordering::SeqCst), 6);
        assert_eq!(releases.load(Ordering::SeqCst), 6);
        assert_eq!(
            *moves.lock().unwrap(),
            vec![
                points[0], points[1], points[2], points[0], points[1], points[2]
            ]
        );

        shared.quit();
        handle.join().unwrap();
    }

    #[test]
    fn sequence_without_points_stops_with_an_error() {
        let settings = Settings::new(50.0, Button::Left, 1.0, 0.0)
            .unwrap()
            .with_mode(ClickMode::Sequence);
        let (shared, presses, _releases, _moves, handle) = spawn_clicker(settings);

        let deadline = Instant::now() + Duration::from_secs(2);
        while shared.status().enabled && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(presses.load(Ordering::SeqCst), 0);
        assert_eq!(shared.status().stop_reason, Some(StopReason::Manual));
        assert!(
            shared
                .status()
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("no points"))
        );

        shared.quit();
        handle.join().unwrap();
    }

    #[test]
    fn sequence_interval_sets_the_period() {
        let settings = Settings::new(50.0, Button::Left, 0.2, 0.0)
            .unwrap()
            .with_mode(ClickMode::Sequence)
            .with_points(&[Point::new(1, 1), Point::new(2, 2)])
            .unwrap()
            .with_point_interval_ms(250)
            .unwrap()
            .with_limit(2)
            .unwrap();
        let (shared, presses, _releases, _moves, handle) = spawn_clicker(settings);
        let started = Instant::now();

        let deadline = Instant::now() + Duration::from_secs(3);
        while shared.status().enabled && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(presses.load(Ordering::SeqCst), 2);
        assert!(started.elapsed() >= Duration::from_millis(230));

        shared.quit();
        handle.join().unwrap();
    }
}
