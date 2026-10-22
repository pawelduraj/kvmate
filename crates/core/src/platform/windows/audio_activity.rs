//! Audio sessions of other programs. Playing means a session is louder than silence; recording
//! means a capture session is running, which Windows shows as "microphone in use".

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{
    AudioSessionStateActive, DEVICE_STATE_ACTIVE, EDataFlow, IAudioSessionControl2, IAudioSessionManager2, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole, eRender,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, STGM_READ,
};
use windows::core::Interface;

use crate::audio::activity::LocalActivity;

const SOUND: f32 = 0.001;

pub struct Probe {
    enumerator: IMMDeviceEnumerator,
    output: Option<IAudioSessionManager2>,
    input: Option<IAudioSessionManager2>,
    own_pid: u32,
}

// SAFETY (all blocks): COM calls in the multithreaded apartment this thread joined, on interfaces
// those calls returned. Strings from PropVariantToStringAlloc are freed with CoTaskMemFree,
// property values with PropVariantClear.
impl Probe {
    pub fn new() -> Option<Probe> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            return Some(Probe { enumerator, output: None, input: None, own_pid: std::process::id() });
        }
    }

    pub fn resolve(&mut self, output: Option<&str>, input: Option<&str>) {
        self.output = self.find(eRender, output).and_then(|d| unsafe { return d.Activate(CLSCTX_ALL, None).ok() });
        self.input = self.find(eCapture, input).and_then(|d| unsafe { return d.Activate(CLSCTX_ALL, None).ok() });
    }

    fn find(&self, flow: EDataFlow, wanted: Option<&str>) -> Option<IMMDevice> {
        unsafe {
            let Some(wanted) = wanted else {
                return self.enumerator.GetDefaultAudioEndpoint(flow, eConsole).ok();
            };
            let all = self.enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE).ok()?;
            let devices = (0..all.GetCount().ok()?)
                .filter_map(|i| return all.Item(i).ok())
                .filter_map(|d| return Some((friendly_name(&d)?, d)));
            return crate::audio::pick_device(devices, wanted);
        }
    }

    pub fn sample(&self) -> LocalActivity {
        let playing = self.output.as_ref().is_some_and(|m| {
            return self.any_session(m, |s| {
                return s
                    .cast::<IAudioMeterInformation>()
                    .ok()
                    .and_then(|meter| unsafe { return meter.GetPeakValue().ok() })
                    .is_some_and(|p| return p > SOUND);
            });
        });
        let recording = self.input.as_ref().is_some_and(|m| {
            return self.any_session(m, |s| {
                return unsafe { s.GetState() }.is_ok_and(|state| return state == AudioSessionStateActive);
            });
        });
        return LocalActivity { playing, recording };
    }

    fn any_session(&self, manager: &IAudioSessionManager2, test: impl Fn(&IAudioSessionControl2) -> bool) -> bool {
        unsafe {
            let Ok(sessions) = manager.GetSessionEnumerator() else { return false };
            let count = sessions.GetCount().unwrap_or(0);
            return (0..count)
                .filter_map(|i| return sessions.GetSession(i).ok())
                .filter_map(|s| return s.cast::<IAudioSessionControl2>().ok())
                .any(|s| return s.GetProcessId().is_ok_and(|pid| return pid != self.own_pid) && test(&s));
        }
    }
}

fn friendly_name(device: &IMMDevice) -> Option<String> {
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let mut value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let text = PropVariantToStringAlloc(&value).ok();
        let _ = PropVariantClear(&mut value);
        let text = text?;
        let name = text.to_string().ok();
        CoTaskMemFree(Some(text.0 as _));
        return name;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn live_activity() {
        let probe = {
            let mut p = super::Probe::new().expect("COM");
            p.resolve(None, None);
            p
        };
        for _ in 0..20 {
            println!("{:?}", probe.sample());
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
}
