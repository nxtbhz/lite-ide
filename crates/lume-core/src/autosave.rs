use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct AutoSaveConfig {
    pub enabled: bool,
    pub debounce: Duration,
}

impl Default for AutoSaveConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce: Duration::from_millis(1000),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AutoSaveController {
    pub config: AutoSaveConfig,
    last_change: Option<Instant>,
}

impl Default for AutoSaveController {
    fn default() -> Self {
        Self::new(AutoSaveConfig::default())
    }
}

impl AutoSaveController {
    pub fn new(config: AutoSaveConfig) -> Self {
        Self {
            config,
            last_change: None,
        }
    }

    pub fn notify_change(&mut self) {
        self.last_change = Some(Instant::now());
    }

    pub fn should_autosave(&self, is_dirty: bool) -> bool {
        if !self.config.enabled || !is_dirty {
            return false;
        }
        if let Some(last) = self.last_change {
            last.elapsed() >= self.config.debounce
        } else {
            false
        }
    }

    pub fn mark_saved(&mut self) {
        self.last_change = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autosave_debounce() {
        let mut controller = AutoSaveController::new(AutoSaveConfig {
            enabled: true,
            debounce: Duration::from_millis(50),
        });

        assert!(!controller.should_autosave(false));
        assert!(!controller.should_autosave(true));

        controller.notify_change();
        // Immediately after, debounce not elapsed yet
        assert!(!controller.should_autosave(true));

        std::thread::sleep(Duration::from_millis(60));
        assert!(controller.should_autosave(true));

        controller.mark_saved();
        assert!(!controller.should_autosave(true));
    }
}
