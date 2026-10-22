use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamMode {
    Off,
    #[default]
    Always,
    WhenLaptop,
}

impl StreamMode {
    pub fn allowed(self, on_laptop: bool) -> bool {
        match self {
            StreamMode::Off => return false,
            StreamMode::Always => return true,
            StreamMode::WhenLaptop => return on_laptop,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub playback: StreamMode,
    pub capture: StreamMode,
    pub output_device: Option<String>,
    pub input_device: Option<String>,
    pub playback_buffer_ms: u32,
    pub capture_buffer_ms: u32,
}

#[cfg(feature = "audio-devices")]
impl AudioConfig {
    pub(super) fn playback_buffer(&self) -> u32 {
        return self.playback_buffer_ms.clamp(10, 200);
    }

    pub(super) fn capture_buffer(&self) -> u32 {
        return self.capture_buffer_ms.clamp(10, 50);
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        return AudioConfig {
            playback: StreamMode::Always,
            capture: StreamMode::Always,
            output_device: None,
            input_device: None,
            playback_buffer_ms: 30,
            capture_buffer_ms: 25,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_mode_allowed() {
        assert!(!StreamMode::Off.allowed(true));
        assert!(StreamMode::Always.allowed(false));
        assert!(StreamMode::WhenLaptop.allowed(true));
        assert!(!StreamMode::WhenLaptop.allowed(false));
    }

    #[test]
    fn fields_round_trip() {
        let c: AudioConfig = serde_json::from_str(r#"{"playback":"when_laptop","capture":"off"}"#).unwrap();
        assert_eq!(c.playback, StreamMode::WhenLaptop);
        assert_eq!(c.capture, StreamMode::Off);
        assert_eq!((c.playback_buffer_ms, c.capture_buffer_ms), (30, 25));
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""playback":"when_laptop""#), "{json}");
        assert_eq!(serde_json::from_str::<AudioConfig>(&json).unwrap(), c);
    }

    #[test]
    fn empty_config_is_default() {
        assert_eq!(serde_json::from_str::<AudioConfig>("{}").unwrap(), AudioConfig::default());
    }
}
