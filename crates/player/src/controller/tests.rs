use std::sync::Mutex;

use super::*;
use crate::player::Media;

/// Records calls instead of playing.
#[derive(Default)]
struct FakePlayer {
    position: Mutex<f64>,
    paused: Mutex<bool>,
    volume: Mutex<f64>,
}

impl Player for FakePlayer {
    fn load(&self, _: Media, start: f64, play: bool) -> Result<(), PlayerError> {
        *self.position.lock().unwrap() = start;
        *self.paused.lock().unwrap() = !play;
        Ok(())
    }
    fn play(&self) -> Result<(), PlayerError> {
        *self.paused.lock().unwrap() = false;
        Ok(())
    }
    fn pause(&self) -> Result<(), PlayerError> {
        *self.paused.lock().unwrap() = true;
        Ok(())
    }
    fn seek(&self, position: f64) -> Result<(), PlayerError> {
        *self.position.lock().unwrap() = position;
        Ok(())
    }
    fn set_speed(&self, _: f64) -> Result<(), PlayerError> {
        Ok(())
    }
    fn set_volume(&self, percent: f64) -> Result<(), PlayerError> {
        *self.volume.lock().unwrap() = percent;
        Ok(())
    }
    fn set_http_headers(&self, _: &[String]) -> Result<(), PlayerError> {
        Ok(())
    }
    fn stop(&self) -> Result<(), PlayerError> {
        Ok(())
    }
    fn position(&self) -> f64 {
        *self.position.lock().unwrap()
    }
    fn is_paused(&self) -> bool {
        *self.paused.lock().unwrap()
    }
}

fn controller() -> (Controller, Arc<FakePlayer>) {
    let fake = Arc::new(FakePlayer::default());
    let mut controller = Controller::new(fake.clone());
    let chapters = [(0.0, 60.0), (60.0, 120.0)]
        .iter()
        .map(|&(start, end)| Chapter {
            start,
            end,
            title: String::new(),
        })
        .collect();
    controller.set_item(chapters, 120.0);
    (controller, fake)
}

#[test]
fn skips_and_chapters_clamp_to_the_item() {
    let (controller, fake) = controller();
    controller.skip_back().unwrap();
    assert_eq!(fake.position(), 0.0);
    controller.next_chapter().unwrap();
    assert_eq!(fake.position(), 60.0);
    controller.next_chapter().unwrap();
    assert_eq!(fake.position(), 60.0, "no chapter after the last");
    controller.seek(119.0).unwrap();
    controller.skip_forward().unwrap();
    assert_eq!(fake.position(), 120.0);
    controller.previous_chapter().unwrap();
    assert_eq!(fake.position(), 60.0);
}

#[test]
fn sleep_timer_fades_then_pauses_and_restores_volume() {
    let (mut controller, fake) = controller();
    let start = Instant::now();
    fake.play().unwrap();
    controller
        .set_sleep(SleepChoice::After(Duration::from_secs(20)), start)
        .unwrap();
    assert!(!controller.tick(start + Duration::from_secs(15)).unwrap());
    assert_eq!(*fake.volume.lock().unwrap(), 50.0);
    assert!(controller.tick(start + Duration::from_secs(21)).unwrap());
    assert!(fake.is_paused());
    assert_eq!(*fake.volume.lock().unwrap(), 100.0);
    assert_eq!(controller.sleep_remaining(start), None);
}

#[test]
fn end_of_chapter_timer_uses_current_chapter() {
    let (mut controller, fake) = controller();
    fake.seek(30.0).unwrap();
    controller
        .set_sleep(SleepChoice::EndOfChapter, Instant::now())
        .unwrap();
    let left = controller.sleep_remaining(Instant::now()).unwrap();
    assert_eq!(left, Duration::from_secs(30));
}
